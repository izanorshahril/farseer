//! Durable work, harness-session references, and rebuildable transcript projections.
//!
//! `40 work model and session explorer` keeps these rows beside the event log,
//! not inside it: work is mutable operator state, session references are
//! observations, and similarity is disposable derived data.

use rusqlite::OptionalExtension;
use serde::Serialize;
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use farseer_core::{
    Actor, Conversation, ConversationId, HarnessSession, RunId, Task, TaskId, TaskState,
    TaskTransition, TranscriptCustody,
};

use crate::{Result, Store, StoreError, uuid_bytes};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(i64::MAX as u128) as i64
        })
}

#[derive(Debug, Clone, Default)]
pub struct TaskFilter<'a> {
    pub conversation_id: Option<ConversationId>,
    pub project_path: Option<&'a str>,
    pub state: Option<TaskState>,
    pub limit: usize,
    pub after: Option<TaskCursor>,
}

/// The last row in the stable task ordering used by bounded board reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskCursor {
    pub priority: i32,
    pub updated_ts: i64,
    pub task_id: TaskId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunParent {
    pub run_id: RunId,
    pub parent_run_id: RunId,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptAttachment {
    pub digest: String,
    pub run_id: RunId,
    pub custody: TranscriptCustody,
    pub source: String,
    pub stored_path: Option<String>,
    pub created_ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TranscriptProjection {
    pub status: String,
    pub error: Option<String>,
    pub coverage: String,
    pub updated_ts: i64,
}

/// Mutable supervision/provenance for a staged artifact.
/// The deterministic artifact bytes are kept at `final_path` only after the
/// worker has completed successfully.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactRow {
    pub artifact_id: RunId,
    pub task_id: TaskId,
    pub run_id: RunId,
    pub kind: String,
    pub status: String,
    pub input_path: String,
    pub staged_path: String,
    pub final_path: Option<String>,
    pub error: Option<String>,
    pub created_ts: i64,
    pub finished_ts: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedTranscript {
    pub digest: String,
    pub body: String,
    pub projection_version: String,
}

/// One bounded explorer row from `40 work model and session explorer`, joining
/// provider-owned session identity to the run and project it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionRow {
    pub session: HarnessSession,
    pub task_id: TaskId,
    pub runner: String,
    pub model: String,
    pub project_path: Option<String>,
    pub log_available: bool,
    pub log_status: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SimilarityEdge {
    pub left_digest: String,
    pub right_digest: String,
    pub score: f64,
    pub embedding_model: String,
    pub dimensions: u32,
    pub distance_metric: String,
    pub redaction_version: String,
    pub projection_version: String,
    pub source_digest: String,
    pub evidence: Vec<String>,
}

/// `04 scoped orchestration graph` keeps this projection bounded and additive.
/// The key is stable within the record and
/// `target` points at the existing operator subject when one exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub project_path: Option<String>,
    pub runner: Option<String>,
    pub target: Option<String>,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub source: Option<String>,
    pub projection: Option<String>,
    pub score: Option<f64>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GraphFilter {
    pub project_path: Option<String>,
    pub runner: Option<String>,
    pub from_ts: Option<i64>,
    pub to_ts: Option<i64>,
    pub kinds: Vec<String>,
    pub offset: usize,
    pub node_limit: usize,
    pub edge_limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GraphPage {
    pub nodes: Vec<GraphNode>,
    pub observed_edges: Vec<GraphEdge>,
    pub derived_edges: Vec<GraphEdge>,
    pub has_more: bool,
}

impl Store {
    pub fn create_artifact(&self, artifact: &ArtifactRow) -> Result<()> {
        self.conn().execute(
            "INSERT INTO artifacts
             (artifact_id, task_id, run_id, kind, status, input_path, staged_path,
              final_path, error, created_ts, finished_ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                &artifact.artifact_id.as_bytes()[..],
                &artifact.task_id.as_bytes()[..],
                &artifact.run_id.as_bytes()[..],
                &artifact.kind,
                &artifact.status,
                &artifact.input_path,
                &artifact.staged_path,
                artifact.final_path.as_deref(),
                artifact.error.as_deref(),
                artifact.created_ts,
                artifact.finished_ts,
            ],
        )?;
        Ok(())
    }

    pub fn update_artifact(
        &self,
        artifact_id: RunId,
        status: &str,
        final_path: Option<&str>,
        error: Option<&str>,
        finished_ts: Option<i64>,
    ) -> Result<bool> {
        Ok(self.conn().execute(
            "UPDATE artifacts SET status = ?2, final_path = ?3, error = ?4,
             finished_ts = ?5 WHERE artifact_id = ?1",
            rusqlite::params![
                &artifact_id.as_bytes()[..],
                status,
                final_path,
                error,
                finished_ts,
            ],
        )? == 1)
    }

    pub fn artifacts_for_task(&self, task_id: TaskId) -> Result<Vec<ArtifactRow>> {
        let mut statement = self.conn().prepare_cached(
            "SELECT artifact_id, run_id, kind, status, input_path, staged_path,
                    final_path, error, created_ts, finished_ts
             FROM artifacts WHERE task_id = ?1 ORDER BY created_ts, rowid",
        )?;
        let rows = statement.query_map([&task_id.as_bytes()[..]], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<i64>>(9)?,
            ))
        })?;
        rows.map(|row| {
            let row = row?;
            Ok(ArtifactRow {
                artifact_id: RunId::from_bytes(uuid_bytes(&row.0, "artifact_id")?),
                task_id,
                run_id: RunId::from_bytes(uuid_bytes(&row.1, "run_id")?),
                kind: row.2,
                status: row.3,
                input_path: row.4,
                staged_path: row.5,
                final_path: row.6,
                error: row.7,
                created_ts: row.8,
                finished_ts: row.9,
            })
        })
        .collect()
    }

    pub fn create_conversation(&self, conversation: &Conversation) -> Result<()> {
        self.conn().execute(
            "INSERT INTO conversations
               (conversation_id, title, project_path, manager_runner, created_ts, updated_ts, archived_ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                &conversation.conversation_id.as_bytes()[..],
                conversation.title,
                conversation.project_path,
                conversation.manager_runner,
                conversation.created_ts,
                conversation.updated_ts,
                conversation.archived_ts,
            ],
        )?;
        Ok(())
    }

    pub fn conversation(&self, id: ConversationId) -> Result<Option<Conversation>> {
        let row = self
            .conn()
            .query_row(
                "SELECT title, project_path, manager_runner, created_ts, updated_ts, archived_ts
                 FROM conversations WHERE conversation_id = ?1",
                [&id.as_bytes()[..]],
                |row| {
                    Ok(Conversation {
                        conversation_id: id,
                        title: row.get(0)?,
                        project_path: row.get(1)?,
                        manager_runner: row.get(2)?,
                        created_ts: row.get(3)?,
                        updated_ts: row.get(4)?,
                        archived_ts: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    pub fn conversations(&self, limit: usize) -> Result<Vec<Conversation>> {
        let mut statement = self.conn().prepare_cached(
            "SELECT conversation_id, title, project_path, manager_runner, created_ts, updated_ts, archived_ts
             FROM conversations ORDER BY updated_ts DESC, rowid DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit as i64], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        })?;
        rows.map(|row| {
            let row = row?;
            Ok(Conversation {
                conversation_id: ConversationId::from_bytes(uuid_bytes(&row.0, "conversation_id")?),
                title: row.1,
                project_path: row.2,
                manager_runner: row.3,
                created_ts: row.4,
                updated_ts: row.5,
                archived_ts: row.6,
            })
        })
        .collect()
    }

    pub fn set_conversation_runner(
        &self,
        id: ConversationId,
        runner: &str,
        ts: i64,
    ) -> Result<bool> {
        Ok(self.conn().execute(
            "UPDATE conversations SET manager_runner = ?2, updated_ts = ?3 WHERE conversation_id = ?1",
            rusqlite::params![&id.as_bytes()[..], runner, ts],
        )? == 1)
    }

    pub fn create_task(&self, task: &Task) -> Result<()> {
        self.conn().execute(
            "INSERT INTO tasks
               (task_id, conversation_id, goal, title, project_path, state, priority, created_ts, updated_ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                &task.task_id.as_bytes()[..],
                &task.conversation_id.as_bytes()[..],
                task.goal,
                task.title,
                task.project_path,
                task.state.as_str(),
                task.priority,
                task.created_ts,
                task.updated_ts,
            ],
        )?;
        self.conn().execute(
            "UPDATE conversations SET updated_ts = MAX(updated_ts, ?2) WHERE conversation_id = ?1",
            rusqlite::params![&task.conversation_id.as_bytes()[..], task.updated_ts],
        )?;
        Ok(())
    }

    pub fn task(&self, id: TaskId) -> Result<Option<Task>> {
        self.conn()
            .query_row(
                "SELECT conversation_id, goal, title, project_path, state, priority, created_ts, updated_ts
                 FROM tasks WHERE task_id = ?1",
                [&id.as_bytes()[..]],
                |row| {
                    let conversation = row.get::<_, Vec<u8>>(0)?;
                    let state = row.get::<_, String>(4)?;
                    Ok((conversation, state, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, i32>(5)?, row.get::<_, i64>(6)?, row.get::<_, i64>(7)?))
                },
            )
            .optional()?
            .map(|row| {
                Ok(Task {
                    task_id: id,
                    conversation_id: ConversationId::from_bytes(uuid_bytes(&row.0, "conversation_id")?),
                    goal: row.2,
                    title: row.3,
                    project_path: row.4,
                    state: row.1.parse().map_err(|error: farseer_core::UnknownTaskState| StoreError::Corrupt { field: "task.state", value: error.0 })?,
                    priority: row.5,
                    created_ts: row.6,
                    updated_ts: row.7,
                })
            })
            .transpose()
    }

    pub fn tasks(&self, filter: &TaskFilter<'_>) -> Result<Vec<Task>> {
        self.select_tasks(filter, false)
    }

    /// Read one bounded keyset page, fetching one sentinel row to report whether
    /// another page exists.
    pub fn task_page(&self, filter: &TaskFilter<'_>) -> Result<Vec<Task>> {
        self.select_tasks(filter, true)
    }

    fn select_tasks(&self, filter: &TaskFilter<'_>, with_sentinel: bool) -> Result<Vec<Task>> {
        let mut sql = String::from("SELECT task_id FROM tasks WHERE 1 = 1");
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(id) = filter.conversation_id {
            values.push(Box::new(id.as_bytes().to_vec()));
            sql.push_str(&format!(" AND conversation_id = ?{}", values.len()));
        }
        if let Some(project) = filter.project_path {
            values.push(Box::new(project.to_owned()));
            sql.push_str(&format!(" AND project_path = ?{}", values.len()));
        }
        if let Some(state) = filter.state {
            values.push(Box::new(state.as_str().to_owned()));
            sql.push_str(&format!(" AND state = ?{}", values.len()));
        }
        if let Some(after) = filter.after {
            values.push(Box::new(after.priority));
            let priority = values.len();
            values.push(Box::new(after.updated_ts));
            let updated = values.len();
            values.push(Box::new(after.task_id.as_bytes().to_vec()));
            let task_id = values.len();
            sql.push_str(&format!(
                " AND (priority < ?{priority} OR (priority = ?{priority} AND (updated_ts < ?{updated} OR (updated_ts = ?{updated} AND task_id < ?{task_id}))))"
            ));
        }
        let limit = filter
            .limit
            .max(1)
            .saturating_add(usize::from(with_sentinel));
        values.push(Box::new(limit.min(i64::MAX as usize) as i64));
        sql.push_str(&format!(
            " ORDER BY priority DESC, updated_ts DESC, task_id DESC LIMIT ?{}",
            values.len()
        ));
        let mut statement = self.conn().prepare(&sql)?;
        let ids = statement
            .query_map(
                rusqlite::params_from_iter(values.iter().map(|value| value.as_ref())),
                |row| row.get::<_, Vec<u8>>(0),
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|bytes| {
                let id = TaskId::from_bytes(uuid_bytes(&bytes, "task_id")?);
                self.task(id)?.ok_or(StoreError::NoSuchTask(id))
            })
            .collect()
    }

    pub fn transition_task(
        &self,
        task_id: TaskId,
        to: TaskState,
        actor: Actor,
        reason: &str,
        ts: i64,
    ) -> Result<TaskTransition> {
        let from = self
            .task(task_id)?
            .ok_or(StoreError::NoSuchTask(task_id))?
            .state;
        if !from.allows(to) {
            return Err(StoreError::InvalidTaskTransition { from, to });
        }
        let transaction = self.conn().unchecked_transaction()?;
        transaction.execute(
            "UPDATE tasks SET state = ?2, updated_ts = ?3 WHERE task_id = ?1",
            rusqlite::params![&task_id.as_bytes()[..], to.as_str(), ts],
        )?;
        transaction.execute(
            "INSERT INTO task_transitions (task_id, from_state, to_state, actor, reason, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                &task_id.as_bytes()[..],
                from.as_str(),
                to.as_str(),
                actor.as_str(),
                reason,
                ts
            ],
        )?;
        transaction.commit()?;
        Ok(TaskTransition {
            task_id,
            from,
            to,
            actor,
            reason: reason.to_owned(),
            ts,
        })
    }

    pub fn task_transitions(&self, task_id: TaskId) -> Result<Vec<TaskTransition>> {
        let mut statement = self.conn().prepare_cached(
            "SELECT from_state, to_state, actor, reason, ts FROM task_transitions
             WHERE task_id = ?1 ORDER BY seq",
        )?;
        let rows = statement.query_map([&task_id.as_bytes()[..]], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })?;
        rows.map(|row| {
            let row = row?;
            Ok(TaskTransition {
                task_id,
                from: row
                    .0
                    .parse()
                    .map_err(
                        |error: farseer_core::UnknownTaskState| StoreError::Corrupt {
                            field: "task_transition.from",
                            value: error.0,
                        },
                    )?,
                to: row
                    .1
                    .parse()
                    .map_err(
                        |error: farseer_core::UnknownTaskState| StoreError::Corrupt {
                            field: "task_transition.to",
                            value: error.0,
                        },
                    )?,
                actor: row
                    .2
                    .parse()
                    .map_err(
                        |error: farseer_core::event::UnknownActor| StoreError::Corrupt {
                            field: "task_transition.actor",
                            value: error.0,
                        },
                    )?,
                reason: row.3,
                ts: row.4,
            })
        })
        .collect()
    }

    pub fn latest_run_for_conversation(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Option<RunId>> {
        let bytes = self.conn().query_row(
            "SELECT runs.run_id FROM runs
             JOIN tasks ON tasks.task_id = runs.task_id
             LEFT JOIN run_parents ON run_parents.run_id = runs.run_id
             WHERE tasks.conversation_id = ?1
               AND (run_parents.kind IS NULL OR run_parents.kind IN ('rerun', 'rescope', 'continuation'))
             ORDER BY runs.started_ts DESC, runs.rowid DESC LIMIT 1",
            [&conversation_id.as_bytes()[..]],
            |row| row.get::<_, Vec<u8>>(0),
        ).optional()?;
        bytes
            .map(|bytes| Ok(RunId::from_bytes(uuid_bytes(&bytes, "run_id")?)))
            .transpose()
    }

    pub fn runs_for_task(&self, task_id: TaskId) -> Result<Vec<crate::RunRow>> {
        let mut statement = self.conn().prepare_cached(
            "SELECT run_id FROM runs WHERE task_id = ?1 ORDER BY started_ts, rowid",
        )?;
        let ids = statement
            .query_map([&task_id.as_bytes()[..]], |row| row.get::<_, Vec<u8>>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|bytes| {
                let id = RunId::from_bytes(uuid_bytes(&bytes, "run_id")?);
                self.run(id)?.ok_or_else(|| StoreError::Corrupt {
                    field: "runs.run_id",
                    value: id.to_string(),
                })
            })
            .collect()
    }

    pub fn record_run_parent(&self, run_id: RunId, parent: RunId, kind: &str) -> Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO run_parents (run_id, parent, kind) VALUES (?1, ?2, ?3)",
            rusqlite::params![&run_id.as_bytes()[..], &parent.as_bytes()[..], kind],
        )?;
        Ok(())
    }

    pub fn run_parents(&self) -> Result<Vec<RunParent>> {
        let mut statement = self
            .conn()
            .prepare_cached("SELECT run_id, parent, kind FROM run_parents ORDER BY rowid")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let row = row?;
            Ok(RunParent {
                run_id: RunId::from_bytes(uuid_bytes(&row.0, "run_parent.run_id")?),
                parent_run_id: RunId::from_bytes(uuid_bytes(&row.1, "run_parent.parent")?),
                kind: row.2,
            })
        })
        .collect()
    }

    /// Read a bounded graph page from the canonical work rows, per `04 scoped
    /// orchestration graph`.
    ///
    /// The union is ordered before the limit so the caller can continue with
    /// an opaque offset without loading the archive into memory.  Edges are
    /// assembled only for the returned nodes and are split by provenance.
    pub fn graph_page(&self, filter: &GraphFilter) -> Result<GraphPage> {
        let mut sql = String::from(
            r#"WITH nodes(kind, id, label, project_path, runner, ts, target, parent) AS (
             SELECT 'project', 'project:' || project_path, project_path, project_path, NULL, MAX(updated_ts), NULL, NULL
               FROM (SELECT project_path, updated_ts FROM conversations WHERE project_path IS NOT NULL
                     UNION ALL SELECT project_path, updated_ts FROM tasks WHERE project_path IS NOT NULL)
              GROUP BY project_path
             UNION ALL
             SELECT 'conversation', 'conversation:' || lower(hex(conversation_id)), title, project_path, manager_runner, updated_ts, lower(hex(conversation_id)), NULL
               FROM conversations
             UNION ALL
             SELECT 'task', 'task:' || lower(hex(task_id)), title, project_path, NULL, updated_ts, lower(hex(task_id)), 'conversation:' || lower(hex(conversation_id))
               FROM tasks
             UNION ALL
             SELECT 'run', 'run:' || lower(hex(runs.run_id)), runs.runner, tasks.project_path, runs.runner, runs.started_ts, lower(hex(runs.run_id)), 'task:' || lower(hex(runs.task_id))
               FROM runs JOIN tasks ON tasks.task_id = runs.task_id
             UNION ALL
             SELECT 'session', 'session:' || lower(hex(harness_sessions.run_id)) || ':' || identifier_kind || ':' || identifier, identifier_kind || ' ' || identifier, tasks.project_path, runs.runner, observed_ts, lower(hex(harness_sessions.run_id)), 'run:' || lower(hex(harness_sessions.run_id))
               FROM harness_sessions JOIN runs ON runs.run_id = harness_sessions.run_id JOIN tasks ON tasks.task_id = runs.task_id
             UNION ALL
             SELECT 'attachment', 'transcript:' || digest, digest, tasks.project_path, runs.runner, transcript_attachments.created_ts, lower(hex(transcript_attachments.run_id)), 'run:' || lower(hex(transcript_attachments.run_id))
               FROM transcript_attachments JOIN runs ON runs.run_id = transcript_attachments.run_id JOIN tasks ON tasks.task_id = runs.task_id
            ) SELECT kind, id, label, project_path, runner, ts, target, parent FROM nodes WHERE 1 = 1"#,
        );
        let mut values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(project) = &filter.project_path {
            values.push(Box::new(project.clone()));
            sql.push_str(&format!(" AND project_path = ?{}", values.len()));
        }
        if let Some(runner) = &filter.runner {
            values.push(Box::new(runner.clone()));
            sql.push_str(&format!(" AND runner = ?{}", values.len()));
        }
        if let Some(from_ts) = filter.from_ts {
            values.push(Box::new(from_ts));
            sql.push_str(&format!(" AND ts >= ?{}", values.len()));
        }
        if let Some(to_ts) = filter.to_ts {
            values.push(Box::new(to_ts));
            sql.push_str(&format!(" AND ts <= ?{}", values.len()));
        }
        if !filter.kinds.is_empty() {
            sql.push_str(" AND kind IN (");
            for (index, kind) in filter.kinds.iter().enumerate() {
                values.push(Box::new(kind.clone()));
                if index > 0 {
                    sql.push_str(", ");
                }
                sql.push_str(&format!("?{}", values.len()));
            }
            sql.push(')');
        }
        let limit = filter.node_limit.max(1).saturating_add(1);
        values.push(Box::new(limit.min(i64::MAX as usize) as i64));
        let limit_param = values.len();
        values.push(Box::new(filter.offset.min(i64::MAX as usize) as i64));
        let offset_param = values.len();
        sql.push_str(&format!(
            " ORDER BY ts DESC, kind, id LIMIT ?{} OFFSET ?{}",
            limit_param, offset_param
        ));
        let mut statement = self.conn().prepare(&sql)?;
        let rows = statement
            .query_map(
                rusqlite::params_from_iter(values.iter().map(|value| value.as_ref())),
                |row| {
                    Ok(GraphNode {
                        id: row.get(1)?,
                        kind: row.get(0)?,
                        label: row.get::<_, String>(2)?.chars().take(256).collect(),
                        project_path: row.get(3)?,
                        runner: row.get(4)?,
                        target: row.get(6)?,
                        parent: row.get(7)?,
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let has_more = rows.len() > filter.node_limit;
        let nodes = rows.into_iter().take(filter.node_limit).collect::<Vec<_>>();
        let visible: HashSet<_> = nodes.iter().map(|node| node.id.as_str()).collect();
        let mut observed_edges = Vec::new();
        for node in &nodes {
            if let Some(parent) = &node.parent
                && visible.contains(parent.as_str())
            {
                observed_edges.push(GraphEdge {
                    from: parent.clone(),
                    to: node.id.clone(),
                    kind: match node.kind.as_str() {
                        "task" => "task",
                        "run" => "run",
                        "session" => "session",
                        "attachment" => "attachment",
                        _ => "contains",
                    }
                    .to_owned(),
                    source: Some(node.id.clone()),
                    projection: None,
                    score: None,
                    evidence: Vec::new(),
                });
            }
            if matches!(node.kind.as_str(), "conversation" | "task")
                && let Some(project) = &node.project_path
            {
                let project_id = format!("project:{project}");
                if visible.contains(project_id.as_str()) {
                    observed_edges.push(GraphEdge {
                        from: project_id,
                        to: node.id.clone(),
                        kind: "project".into(),
                        source: Some(node.id.clone()),
                        projection: None,
                        score: None,
                        evidence: Vec::new(),
                    });
                }
            }
        }

        let run_ids = nodes
            .iter()
            .filter_map(|node| node.id.strip_prefix("run:"))
            .collect::<Vec<_>>();
        if !run_ids.is_empty() {
            let mut query = String::from(
                "SELECT lower(hex(run_id)), lower(hex(parent)), kind FROM run_parents WHERE run_id IN (",
            );
            let mut run_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            for (index, id) in run_ids.iter().enumerate() {
                let bytes = decode_hex(id).ok_or_else(|| StoreError::Corrupt {
                    field: "run_parent.run_id",
                    value: (*id).to_owned(),
                })?;
                run_values.push(Box::new(bytes));
                if index > 0 {
                    query.push_str(", ");
                }
                query.push_str(&format!("?{}", run_values.len()));
            }
            query.push(')');
            let mut statement = self.conn().prepare(&query)?;
            let parents = statement
                .query_map(
                    rusqlite::params_from_iter(run_values.iter().map(|value| value.as_ref())),
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for (run, parent, kind) in parents {
                let from = format!("run:{parent}");
                let to = format!("run:{run}");
                if visible.contains(from.as_str()) && visible.contains(to.as_str()) {
                    observed_edges.push(GraphEdge {
                        from,
                        to,
                        kind,
                        source: Some(format!("run:{run}")),
                        projection: None,
                        score: None,
                        evidence: Vec::new(),
                    });
                }
            }
        }

        let digests = nodes
            .iter()
            .filter_map(|node| node.id.strip_prefix("transcript:"))
            .collect::<Vec<_>>();
        let mut derived_edges = Vec::new();
        if !digests.is_empty() {
            let mut query = String::from(
                "SELECT left_digest, right_digest, score, projection_version, source_digest, evidence FROM similarity_edges WHERE left_digest IN (",
            );
            let mut edge_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            for (index, digest) in digests.iter().enumerate() {
                edge_values.push(Box::new((*digest).to_owned()));
                if index > 0 {
                    query.push_str(", ");
                }
                query.push_str(&format!("?{}", edge_values.len()));
            }
            query.push_str(") OR right_digest IN (");
            for (index, digest) in digests.iter().enumerate() {
                edge_values.push(Box::new((*digest).to_owned()));
                if index > 0 {
                    query.push_str(", ");
                }
                query.push_str(&format!("?{}", edge_values.len()));
            }
            edge_values.push(Box::new(
                filter.edge_limit.max(1).min(i64::MAX as usize) as i64
            ));
            query.push_str(&format!(
                " ORDER BY score DESC LIMIT ?{}",
                edge_values.len()
            ));
            let mut statement = self.conn().prepare(&query)?;
            let rows = statement
                .query_map(
                    rusqlite::params_from_iter(edge_values.iter().map(|value| value.as_ref())),
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, f64>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    },
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for (left, right, score, projection, source, evidence) in rows {
                let from = format!("transcript:{left}");
                let to = format!("transcript:{right}");
                if visible.contains(from.as_str()) && visible.contains(to.as_str()) {
                    derived_edges.push(GraphEdge {
                        from,
                        to,
                        kind: "similarity".into(),
                        source: Some(source),
                        projection: Some(projection),
                        score: Some(score),
                        evidence: serde_json::from_str::<Vec<String>>(&evidence)?
                            .into_iter()
                            .take(8)
                            .collect(),
                    });
                }
            }
        }
        let total_edges = observed_edges.len().saturating_add(derived_edges.len());
        if total_edges > filter.edge_limit {
            let mut remaining = filter.edge_limit;
            observed_edges.truncate(remaining);
            remaining = remaining.saturating_sub(observed_edges.len());
            derived_edges.truncate(remaining);
        }
        Ok(GraphPage {
            nodes,
            observed_edges,
            derived_edges,
            has_more: has_more || total_edges > filter.edge_limit,
        })
    }

    pub fn observe_harness_session(&self, session: &HarnessSession) -> Result<()> {
        self.conn().execute(
            "INSERT INTO harness_sessions
               (run_id, identifier_kind, identifier, log_pointer, observed_ts)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(run_id, identifier_kind, identifier) DO UPDATE SET
               log_pointer = COALESCE(excluded.log_pointer, harness_sessions.log_pointer)",
            rusqlite::params![
                &session.run_id.as_bytes()[..],
                session.identifier_kind,
                session.identifier,
                session.log_pointer,
                session.observed_ts
            ],
        )?;
        Ok(())
    }

    pub fn harness_sessions(&self, run_id: Option<RunId>) -> Result<Vec<HarnessSession>> {
        let (sql, value): (&str, Option<Vec<u8>>) = match run_id {
            Some(id) => (
                "SELECT run_id, identifier_kind, identifier, log_pointer, observed_ts FROM harness_sessions WHERE run_id = ?1 ORDER BY observed_ts",
                Some(id.as_bytes().to_vec()),
            ),
            None => (
                "SELECT run_id, identifier_kind, identifier, log_pointer, observed_ts FROM harness_sessions ORDER BY observed_ts",
                None,
            ),
        };
        let mut statement = self.conn().prepare(sql)?;
        let map = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        };
        let rows = match value {
            Some(value) => statement
                .query_map([value], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
            None => statement
                .query_map([], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
        };
        rows.into_iter()
            .map(|row| {
                Ok(HarnessSession {
                    run_id: RunId::from_bytes(uuid_bytes(&row.0, "harness_session.run_id")?),
                    identifier_kind: row.1,
                    identifier: row.2,
                    log_pointer: row.3,
                    observed_ts: row.4,
                })
            })
            .collect()
    }

    /// Bounded session explorer rows from `40 work model and session explorer`.
    /// Missing log pointers remain visible and are labelled unavailable rather than guessed from private directories.
    pub fn harness_session_page(
        &self,
        limit: usize,
        offset: usize,
        project_path: Option<&str>,
        conversation_id: Option<ConversationId>,
        task_id: Option<TaskId>,
        run_id: Option<RunId>,
    ) -> Result<(Vec<SessionRow>, Option<usize>)> {
        let limit = limit.clamp(1, 500);
        let mut stmt = self.conn.prepare_cached(
            "SELECT h.run_id, h.identifier_kind, h.identifier, h.log_pointer, h.observed_ts,
                    r.task_id, r.runner, r.model, t.project_path
             FROM harness_sessions h
             JOIN runs r ON r.run_id = h.run_id
             JOIN tasks t ON t.task_id = r.task_id
             JOIN conversations c ON c.conversation_id = t.conversation_id
             WHERE (?1 IS NULL OR t.project_path = ?1)
               AND (?2 IS NULL OR c.conversation_id = ?2)
               AND (?3 IS NULL OR t.task_id = ?3)
               AND (?4 IS NULL OR h.run_id = ?4)
             ORDER BY h.observed_ts DESC, h.run_id DESC
             LIMIT ?5 OFFSET ?6",
        )?;
        let raw = stmt
            .query_map(
                rusqlite::params![
                    project_path,
                    conversation_id.map(|id| id.as_bytes().to_vec()),
                    task_id.map(|id| id.as_bytes().to_vec()),
                    run_id.map(|id| id.as_bytes().to_vec()),
                    (limit + 1) as i64,
                    offset as i64
                ],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Vec<u8>>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let rows = raw
            .into_iter()
            .map(|row| {
                let log_available = row.3.is_some();
                let log_status = match row.3.as_deref() {
                    None => "unavailable",
                    Some(pointer) if pointer.starts_with("rotated:") => "rotated",
                    Some(_) => "referenced",
                };
                Ok(SessionRow {
                    session: HarnessSession {
                        run_id: RunId::from_bytes(uuid_bytes(&row.0, "session.run_id")?),
                        identifier_kind: row.1,
                        identifier: row.2,
                        log_pointer: row.3,
                        observed_ts: row.4,
                    },
                    task_id: TaskId::from_bytes(uuid_bytes(&row.5, "session.task_id")?),
                    runner: row.6,
                    model: row.7,
                    project_path: row.8,
                    log_available,
                    log_status,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let next = (rows.len() > limit).then_some(offset + limit);
        let mut rows = rows;
        if next.is_some() {
            rows.truncate(limit);
        }
        Ok((rows, next))
    }

    pub fn record_transcript_attachment(&self, attachment: &TranscriptAttachment) -> Result<()> {
        self.conn().execute(
            "INSERT INTO transcript_attachments
               (digest, run_id, custody, source, stored_path, created_ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(digest, run_id) DO UPDATE SET
               custody = excluded.custody,
               source = excluded.source,
               stored_path = excluded.stored_path,
               created_ts = excluded.created_ts",
            rusqlite::params![
                attachment.digest,
                &attachment.run_id.as_bytes()[..],
                attachment.custody.as_str(),
                attachment.source,
                attachment.stored_path,
                attachment.created_ts
            ],
        )?;
        if attachment.custody != TranscriptCustody::CopyPlusIndex {
            self.conn().execute(
                "DELETE FROM transcript_projection_jobs WHERE digest = ?1 AND run_id = ?2",
                rusqlite::params![attachment.digest, &attachment.run_id.as_bytes()[..]],
            )?;
        }
        Ok(())
    }

    pub fn queue_transcript_projection(
        &self,
        attachment: &TranscriptAttachment,
        updated_ts: i64,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT INTO transcript_projection_jobs
                (digest, run_id, status, error, coverage, updated_ts)
             VALUES (?1, ?2, 'pending', NULL, 'restricted', ?3)
             ON CONFLICT(digest, run_id) DO UPDATE SET
                status = 'pending', error = NULL, coverage = 'restricted', updated_ts = excluded.updated_ts",
            rusqlite::params![attachment.digest, &attachment.run_id.as_bytes()[..], updated_ts],
        )?;
        Ok(())
    }

    pub fn transcript_projection(
        &self,
        digest: &str,
        run_id: RunId,
    ) -> Result<Option<TranscriptProjection>> {
        self.conn()
            .query_row(
                "SELECT status, error, coverage, updated_ts
                 FROM transcript_projection_jobs WHERE digest = ?1 AND run_id = ?2",
                rusqlite::params![digest, &run_id.as_bytes()[..]],
                |row| {
                    Ok(TranscriptProjection {
                        status: row.get(0)?,
                        error: row.get(1)?,
                        coverage: row.get(2)?,
                        updated_ts: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn pending_transcript_projections(&self) -> Result<Vec<TranscriptAttachment>> {
        let mut statement = self.conn().prepare(
            "SELECT a.digest, a.run_id, a.custody, a.source, a.stored_path, a.created_ts
             FROM transcript_projection_jobs j
             JOIN transcript_attachments a ON a.digest = j.digest AND a.run_id = j.run_id
             WHERE j.status = 'pending' ORDER BY j.updated_ts, a.created_ts",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| {
                Ok(TranscriptAttachment {
                    digest: row.0,
                    run_id: RunId::from_bytes(uuid_bytes(&row.1, "transcript_attachment.run_id")?),
                    custody: row.2.parse().map_err(
                        |error: farseer_core::UnknownTranscriptCustody| StoreError::Corrupt {
                            field: "transcript_attachment.custody",
                            value: error.0,
                        },
                    )?,
                    source: row.3,
                    stored_path: row.4,
                    created_ts: row.5,
                })
            })
            .collect()
    }

    pub fn set_transcript_projection_status(
        &self,
        attachment: &TranscriptAttachment,
        status: &str,
        error: Option<&str>,
        coverage: &str,
        updated_ts: i64,
    ) -> Result<bool> {
        let changed = self.conn().execute(
            "UPDATE transcript_projection_jobs
             SET status = ?3, error = ?4, coverage = ?5, updated_ts = ?6
             WHERE digest = ?1 AND run_id = ?2
               AND EXISTS (
                 SELECT 1 FROM transcript_attachments a
                 WHERE a.digest = ?1 AND a.run_id = ?2
                   AND a.custody = ?7 AND a.source = ?8
                   AND (a.stored_path = ?9 OR (a.stored_path IS NULL AND ?9 IS NULL))
               )",
            rusqlite::params![
                attachment.digest,
                &attachment.run_id.as_bytes()[..],
                status,
                error,
                coverage,
                updated_ts,
                attachment.custody.as_str(),
                attachment.source,
                attachment.stored_path
            ],
        )?;
        Ok(changed == 1)
    }

    pub fn transcript_attachments(
        &self,
        run_id: Option<RunId>,
    ) -> Result<Vec<TranscriptAttachment>> {
        let (sql, value): (&str, Option<Vec<u8>>) = match run_id {
            Some(id) => (
                "SELECT digest, run_id, custody, source, stored_path, created_ts FROM transcript_attachments WHERE run_id = ?1 ORDER BY created_ts",
                Some(id.as_bytes().to_vec()),
            ),
            None => (
                "SELECT digest, run_id, custody, source, stored_path, created_ts FROM transcript_attachments ORDER BY created_ts",
                None,
            ),
        };
        let mut statement = self.conn().prepare(sql)?;
        let map = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, i64>(5)?,
            ))
        };
        let rows = match value {
            Some(value) => statement
                .query_map([value], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
            None => statement
                .query_map([], map)?
                .collect::<std::result::Result<Vec<_>, _>>()?,
        };
        rows.into_iter()
            .map(|row| {
                Ok(TranscriptAttachment {
                    digest: row.0,
                    run_id: RunId::from_bytes(uuid_bytes(&row.1, "transcript_attachment.run_id")?),
                    custody: row.2.parse().map_err(
                        |error: farseer_core::UnknownTranscriptCustody| StoreError::Corrupt {
                            field: "transcript_attachment.custody",
                            value: error.0,
                        },
                    )?,
                    source: row.3,
                    stored_path: row.4,
                    created_ts: row.5,
                })
            })
            .collect()
    }

    pub fn index_transcript(
        &self,
        digest: &str,
        body: &str,
        redaction: &str,
        projection: &str,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT OR REPLACE INTO transcript_index (digest, body, redaction_version, projection_version)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![digest, body, redaction, projection],
        )?;
        Ok(())
    }

    /// Read only a bounded candidate set and bounded UTF-8 prefixes.
    ///
    /// The byte budget is applied to the bytes returned by SQLite, so a large
    /// collection of large projections cannot be materialized before the
    /// caller gets a chance to stop.
    pub fn indexed_transcripts_bounded(
        &self,
        candidate_limit: usize,
        byte_limit: usize,
    ) -> Result<Vec<(String, String)>> {
        if candidate_limit == 0 || byte_limit == 0 {
            return Ok(Vec::new());
        }
        let digests = {
            let mut statement = self
                .conn()
                .prepare_cached("SELECT digest FROM transcript_index ORDER BY digest LIMIT ?1")?;
            statement
                .query_map([candidate_limit.min(i64::MAX as usize) as i64], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let mut used = 0usize;
        let mut documents = Vec::with_capacity(digests.len());
        for digest in digests {
            let remaining = byte_limit.saturating_sub(used);
            if remaining == 0 {
                break;
            }
            let bytes = self.conn().query_row(
                "SELECT substr(CAST(body AS BLOB), 1, ?2) FROM transcript_index WHERE digest = ?1",
                rusqlite::params![digest, remaining.min(i64::MAX as usize) as i64],
                |row| row.get::<_, Vec<u8>>(0),
            )?;
            let body = valid_utf8_prefix(bytes);
            used = used.saturating_add(body.len());
            documents.push((digest, body));
        }
        Ok(documents)
    }

    /// Read one bounded page of scrubbed transcript projections with the
    /// version that produced each row.
    pub fn indexed_transcript_page(
        &self,
        limit: usize,
        offset: usize,
        byte_limit: usize,
    ) -> Result<(Vec<IndexedTranscript>, Option<usize>)> {
        self.indexed_transcript_page_matching(None, limit, offset, byte_limit)
    }

    /// Read a bounded page of scrubbed transcript projections that match a
    /// query in SQLite before pagination, so an offset cannot skip matching
    /// documents hidden among non-matching rows.
    pub fn indexed_transcript_search_page(
        &self,
        needle: &str,
        limit: usize,
        offset: usize,
        byte_limit: usize,
    ) -> Result<(Vec<IndexedTranscript>, Option<usize>)> {
        self.indexed_transcript_page_matching(Some(needle), limit, offset, byte_limit)
    }

    fn indexed_transcript_page_matching(
        &self,
        needle: Option<&str>,
        limit: usize,
        offset: usize,
        byte_limit: usize,
    ) -> Result<(Vec<IndexedTranscript>, Option<usize>)> {
        if limit == 0 || byte_limit == 0 {
            return Ok((Vec::new(), None));
        }
        let limit = limit.min(500);
        let rows = if let Some(needle) = needle {
            let mut statement = self.conn().prepare_cached(
                "SELECT digest, projection_version
                 FROM transcript_index
                 WHERE instr(lower(body), lower(?1)) > 0
                 ORDER BY digest LIMIT ?2 OFFSET ?3",
            )?;
            statement
                .query_map(
                    rusqlite::params![needle, (limit + 1) as i64, offset as i64],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut statement = self.conn().prepare_cached(
                "SELECT digest, projection_version
                 FROM transcript_index ORDER BY digest LIMIT ?1 OFFSET ?2",
            )?;
            statement
                .query_map(
                    rusqlite::params![(limit + 1) as i64, offset as i64],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?
        };
        let has_more = rows.len() > limit;
        let mut documents = Vec::with_capacity(rows.len().min(limit));
        let mut used = 0usize;
        for (digest, projection) in rows.into_iter().take(limit) {
            let remaining = byte_limit.saturating_sub(used);
            if remaining == 0 {
                break;
            }
            let bytes = self.conn().query_row(
                "SELECT substr(CAST(body AS BLOB), 1, ?2) FROM transcript_index WHERE digest = ?1",
                rusqlite::params![digest, remaining.min(i64::MAX as usize) as i64],
                |row| row.get::<_, Vec<u8>>(0),
            )?;
            let body = valid_utf8_prefix(bytes);
            used = used.saturating_add(body.len());
            documents.push(IndexedTranscript {
                digest,
                body,
                projection_version: projection,
            });
        }
        Ok((documents, has_more.then_some(offset + limit)))
    }

    /// Commit one projection and its edges only if its attachment is still the
    /// same row that was read before analysis.
    pub fn commit_transcript_projection(
        &self,
        attachment: &TranscriptAttachment,
        body: &str,
        redaction: &str,
        projection: &str,
        edges: &[SimilarityEdge],
    ) -> Result<()> {
        self.commit_transcript_projection_with_coverage(
            attachment,
            body,
            redaction,
            projection,
            "restricted",
            edges,
        )
    }

    pub fn commit_transcript_projection_with_coverage(
        &self,
        attachment: &TranscriptAttachment,
        body: &str,
        redaction: &str,
        projection: &str,
        coverage: &str,
        edges: &[SimilarityEdge],
    ) -> Result<()> {
        let transaction = self.conn().unchecked_transaction()?;
        let current = transaction
            .query_row(
                "SELECT a.custody, a.source, a.stored_path, j.status
                 FROM transcript_attachments a
                 LEFT JOIN transcript_projection_jobs j
                   ON j.digest = a.digest AND j.run_id = a.run_id
                 WHERE a.digest = ?1 AND a.run_id = ?2",
                rusqlite::params![attachment.digest, &attachment.run_id.as_bytes()[..]],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((custody, source, stored_path, status)) = current else {
            return Err(StoreError::StaleTranscriptProjection);
        };
        if custody != attachment.custody.as_str()
            || source != attachment.source
            || stored_path != attachment.stored_path
        {
            return Err(StoreError::StaleTranscriptProjection);
        }
        if status.as_deref() != Some("pending") {
            return Err(StoreError::StaleTranscriptProjection);
        }
        transaction.execute(
            "INSERT OR REPLACE INTO transcript_index
             (digest, body, redaction_version, projection_version)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![attachment.digest, body, redaction, projection],
        )?;
        transaction.execute(
            "DELETE FROM similarity_edges WHERE left_digest = ?1 OR right_digest = ?1",
            [&attachment.digest],
        )?;
        for edge in edges {
            transaction.execute(
                "INSERT OR REPLACE INTO similarity_edges
                   (left_digest, right_digest, score, embedding_model, dimensions, distance_metric,
                    redaction_version, projection_version, source_digest, evidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    edge.left_digest,
                    edge.right_digest,
                    edge.score,
                    edge.embedding_model,
                    edge.dimensions,
                    edge.distance_metric,
                    edge.redaction_version,
                    edge.projection_version,
                    edge.source_digest,
                    serde_json::to_string(&edge.evidence)?
                ],
            )?;
        }
        transaction.execute(
            "UPDATE transcript_projection_jobs
             SET status = CASE WHEN ?3 = 'truncated' THEN 'truncated' ELSE 'complete' END,
                 error = NULL, coverage = ?3, updated_ts = ?4
             WHERE digest = ?1 AND run_id = ?2 AND status = 'pending'",
            rusqlite::params![
                attachment.digest,
                &attachment.run_id.as_bytes()[..],
                coverage,
                now_ms()
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn indexed_transcripts(&self) -> Result<Vec<(String, String)>> {
        let mut statement = self
            .conn()
            .prepare_cached("SELECT digest, body FROM transcript_index ORDER BY digest")?;
        Ok(statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn replace_similarity_edges(&self, digest: &str, edges: &[SimilarityEdge]) -> Result<()> {
        let transaction = self.conn().unchecked_transaction()?;
        transaction.execute(
            "DELETE FROM similarity_edges WHERE left_digest = ?1 OR right_digest = ?1",
            [digest],
        )?;
        for edge in edges {
            transaction.execute(
                "INSERT OR REPLACE INTO similarity_edges
                   (left_digest, right_digest, score, embedding_model, dimensions, distance_metric,
                    redaction_version, projection_version, source_digest, evidence)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    edge.left_digest,
                    edge.right_digest,
                    edge.score,
                    edge.embedding_model,
                    edge.dimensions,
                    edge.distance_metric,
                    edge.redaction_version,
                    edge.projection_version,
                    edge.source_digest,
                    serde_json::to_string(&edge.evidence)?
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn similarity_edges(&self) -> Result<Vec<SimilarityEdge>> {
        let mut statement = self.conn().prepare_cached(
            "SELECT left_digest, right_digest, score, embedding_model, dimensions, distance_metric,
                    redaction_version, projection_version, source_digest, evidence
             FROM similarity_edges ORDER BY score DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
                row.get::<_, String>(9)?,
            ))
        })?;
        rows.map(|row| {
            let row = row?;
            Ok(SimilarityEdge {
                left_digest: row.0,
                right_digest: row.1,
                score: row.2,
                embedding_model: row.3,
                dimensions: row.4 as u32,
                distance_metric: row.5,
                redaction_version: row.6,
                projection_version: row.7,
                source_digest: row.8,
                evidence: serde_json::from_str(&row.9)?,
            })
        })
        .collect()
    }
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect()
}

fn valid_utf8_prefix(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            let valid = error.utf8_error().valid_up_to();
            String::from_utf8(error.into_bytes()[..valid].to_vec())
                .expect("the prefix reported valid by UTF-8 is valid")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conversation() -> Conversation {
        Conversation {
            conversation_id: ConversationId::new(),
            title: "Ship work".into(),
            project_path: Some("D:/Dev/farseer".into()),
            manager_runner: Some("pi".into()),
            created_ts: 1,
            updated_ts: 1,
            archived_ts: None,
        }
    }

    fn task(conversation_id: ConversationId) -> Task {
        Task {
            task_id: TaskId::new(),
            conversation_id,
            goal: "finish it".into(),
            title: "Finish it".into(),
            project_path: Some("D:/Dev/farseer".into()),
            state: TaskState::Inbox,
            priority: 0,
            created_ts: 1,
            updated_ts: 1,
        }
    }

    #[test]
    fn one_task_is_the_same_row_on_global_and_project_boards() {
        let store = Store::open_in_memory().unwrap();
        let conversation = conversation();
        let task = task(conversation.conversation_id);
        store.create_conversation(&conversation).unwrap();
        store.create_task(&task).unwrap();

        let global = store
            .tasks(&TaskFilter {
                limit: 20,
                ..Default::default()
            })
            .unwrap();
        let project = store
            .tasks(&TaskFilter {
                project_path: task.project_path.as_deref(),
                limit: 20,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(global, project);
        assert_eq!(global, [task]);
    }

    #[test]
    fn task_pages_use_a_stable_keyset_order_and_scope() {
        let store = Store::open_in_memory().unwrap();
        let conversation = conversation();
        store.create_conversation(&conversation).unwrap();
        let mut tasks = Vec::new();
        for (priority, updated_ts, project_path) in [
            (2, 3, Some("D:/one")),
            (2, 2, Some("D:/one")),
            (1, 9, Some("D:/two")),
        ] {
            let mut task = task(conversation.conversation_id);
            task.priority = priority;
            task.updated_ts = updated_ts;
            task.project_path = project_path.map(str::to_owned);
            task.task_id = TaskId::new();
            store.create_task(&task).unwrap();
            tasks.push(task);
        }

        let first = store
            .task_page(&TaskFilter {
                project_path: Some("D:/one"),
                limit: 1,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(first.len(), 2, "one sentinel row reports another page");
        let after = TaskCursor {
            priority: first[0].priority,
            updated_ts: first[0].updated_ts,
            task_id: first[0].task_id,
        };
        let second = store
            .task_page(&TaskFilter {
                project_path: Some("D:/one"),
                limit: 1,
                after: Some(after),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(second[0].task_id, tasks[1].task_id);
        assert_eq!(first[0].project_path.as_deref(), Some("D:/one"));
        assert!(
            second
                .iter()
                .all(|task| task.project_path.as_deref() == Some("D:/one"))
        );
    }

    #[test]
    fn graph_pages_bound_nodes_and_keep_observed_edges_separate() {
        let store = Store::open_in_memory().unwrap();
        let conversation = conversation();
        let task = task(conversation.conversation_id);
        store.create_conversation(&conversation).unwrap();
        store.create_task(&task).unwrap();
        let parent = RunId::new();
        let child = RunId::new();
        for run_id in [parent, child] {
            store
                .upsert_run(&crate::RunRow {
                    run_id,
                    task_id: task.task_id,
                    cell_id: farseer_core::CellId::new("zero"),
                    runner: "pi".into(),
                    model: "model".into(),
                    outcome: Some("ok".into()),
                    usd_micros: 0,
                    tokens: 1,
                    operator_touched: false,
                    started_ts: 2,
                    finished_ts: Some(3),
                })
                .unwrap();
        }
        store
            .record_run_parent(child, parent, "delegation")
            .unwrap();
        let page = store
            .graph_page(&GraphFilter {
                project_path: Some("D:/Dev/farseer".into()),
                node_limit: 3,
                edge_limit: 1,
                ..Default::default()
            })
            .unwrap();
        assert!(page.nodes.len() <= 3);
        assert!(page.has_more);
        assert!(page.derived_edges.is_empty());
        assert!(page.observed_edges.iter().all(|edge| edge.source.is_some()));
    }

    #[test]
    fn transitions_validate_and_retain_actor_and_reason() {
        let store = Store::open_in_memory().unwrap();
        let conversation = conversation();
        let task = task(conversation.conversation_id);
        store.create_conversation(&conversation).unwrap();
        store.create_task(&task).unwrap();

        let changed = store
            .transition_task(
                task.task_id,
                TaskState::InProgress,
                Actor::Operator,
                "accepted",
                2,
            )
            .unwrap();
        assert_eq!(changed.reason, "accepted");
        assert_eq!(changed.actor, Actor::Operator);
        assert!(
            store
                .transition_task(
                    task.task_id,
                    TaskState::Done,
                    Actor::Operator,
                    "skip review",
                    3
                )
                .is_err()
        );
        assert_eq!(store.task_transitions(task.task_id).unwrap(), [changed]);
    }

    #[test]
    fn one_run_keeps_several_provider_session_kinds() {
        let store = Store::open_in_memory().unwrap();
        let run_id = RunId::new();
        for (kind, id) in [("thread", "thread-1"), ("session", "session-2")] {
            store
                .observe_harness_session(&HarnessSession {
                    run_id,
                    identifier_kind: kind.into(),
                    identifier: id.into(),
                    log_pointer: None,
                    observed_ts: 1,
                })
                .unwrap();
        }
        assert_eq!(store.harness_sessions(Some(run_id)).unwrap().len(), 2);
    }
    #[test]
    fn identical_transcript_content_remains_attached_to_each_run() {
        let store = Store::open_in_memory().unwrap();
        let first = RunId::new();
        let second = RunId::new();
        for run_id in [first, second] {
            store
                .record_transcript_attachment(&TranscriptAttachment {
                    digest: "same-content".into(),
                    run_id,
                    custody: TranscriptCustody::Copy,
                    source: "transcript.jsonl".into(),
                    stored_path: Some("objects/same-content".into()),
                    created_ts: 1,
                })
                .unwrap();
        }

        assert_eq!(store.transcript_attachments(Some(first)).unwrap().len(), 1);
        assert_eq!(store.transcript_attachments(Some(second)).unwrap().len(), 1);
    }

    #[test]
    fn bounded_transcript_candidates_cap_rows_and_bytes() {
        let store = Store::open_in_memory().unwrap();
        for (digest, body) in [("a", "12345"), ("b", "67890"), ("c", "extra")] {
            store
                .index_transcript(digest, body, "redaction", "projection")
                .unwrap();
        }
        let rows = store.indexed_transcripts_bounded(2, 7).unwrap();
        assert!(rows.len() <= 2);
        assert!(rows.iter().map(|(_, body)| body.len()).sum::<usize>() <= 7);
    }

    #[test]
    fn transcript_projection_status_is_recoverable_and_cancelled_jobs_cannot_commit() {
        let store = Store::open_in_memory().unwrap();
        let attachment = TranscriptAttachment {
            digest: "digest".into(),
            run_id: RunId::new(),
            custody: TranscriptCustody::CopyPlusIndex,
            source: "transcript.jsonl".into(),
            stored_path: Some("stored".into()),
            created_ts: 1,
        };
        store.record_transcript_attachment(&attachment).unwrap();
        store.queue_transcript_projection(&attachment, 2).unwrap();
        assert_eq!(
            store
                .transcript_projection(&attachment.digest, attachment.run_id)
                .unwrap()
                .unwrap()
                .status,
            "pending"
        );
        store
            .set_transcript_projection_status(&attachment, "cancelled", None, "restricted", 3)
            .unwrap();
        assert!(matches!(
            store.commit_transcript_projection(&attachment, "safe", "redaction", "projection", &[]),
            Err(StoreError::StaleTranscriptProjection)
        ));
        assert!(store.indexed_transcripts().unwrap().is_empty());
    }

    #[test]
    fn indexed_transcript_page_keeps_projection_version_and_cursor_bounded() {
        let store = Store::open_in_memory().unwrap();
        store
            .index_transcript("a", "alpha", "redact-v1", "hash-tf-v1")
            .unwrap();
        store
            .index_transcript("b", "beta", "redact-v1", "hash-tf-v2")
            .unwrap();
        let (rows, next) = store.indexed_transcript_page(1, 0, 64).unwrap();
        assert_eq!(rows[0].projection_version, "hash-tf-v1");
        assert_eq!(next, Some(1));
        let (rows, next) = store.indexed_transcript_page(1, 1, 64).unwrap();
        assert_eq!(rows[0].digest, "b");
        assert_eq!(next, None);
    }

    #[test]
    fn transcript_search_paginates_matches_before_applying_the_offset() {
        let store = Store::open_in_memory().unwrap();
        for (digest, body) in [("a", "unrelated"), ("b", "needle one"), ("c", "needle two")] {
            store
                .index_transcript(digest, body, "redact-v1", "hash-tf-v1")
                .unwrap();
        }
        let (rows, next) = store
            .indexed_transcript_search_page("needle", 1, 0, 64)
            .unwrap();
        assert_eq!(rows[0].digest, "b");
        assert_eq!(next, Some(1));
        let (rows, next) = store
            .indexed_transcript_search_page("needle", 1, 1, 64)
            .unwrap();
        assert_eq!(rows[0].digest, "c");
        assert_eq!(next, None);
    }

    #[test]
    fn projection_commit_rejects_a_replaced_attachment() {
        let store = Store::open_in_memory().unwrap();
        let run_id = RunId::new();
        let old = TranscriptAttachment {
            digest: "digest".into(),
            run_id,
            custody: TranscriptCustody::CopyPlusIndex,
            source: "old.jsonl".into(),
            stored_path: Some("stored".into()),
            created_ts: 1,
        };
        store.record_transcript_attachment(&old).unwrap();
        let mut replacement = old.clone();
        replacement.source = "new.jsonl".into();
        store.record_transcript_attachment(&replacement).unwrap();
        assert!(matches!(
            store.commit_transcript_projection(&old, "safe", "redaction", "projection", &[]),
            Err(crate::StoreError::StaleTranscriptProjection)
        ));
        assert!(store.indexed_transcripts().unwrap().is_empty());
    }
}
