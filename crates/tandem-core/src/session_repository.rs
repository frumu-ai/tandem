use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::{json, Value};
use tandem_types::{Message, MessagePart, MessageRole, Session};

use crate::message_part_reducer::reduce_message_parts;

use super::{QuestionRequest, SessionMeta, MAX_SESSION_SNAPSHOTS};

const JSON_IMPORT_MIGRATION: &str = "sessions_json_import_v1";

/// Migration inputs are retained on disk. The transaction records their digest
/// before making the SQLite database authoritative, so a restart cannot import
/// them a second time.
pub(crate) struct LegacyImportState {
    pub sessions: HashMap<String, Session>,
    pub metadata: HashMap<String, SessionMeta>,
    pub questions: HashMap<String, QuestionRequest>,
    pub sources: Vec<LegacySource>,
}

pub(crate) struct LegacySource {
    pub path: PathBuf,
    pub digest: Option<String>,
}

#[derive(Clone)]
pub(crate) struct SessionRepository {
    database_path: PathBuf,
}

impl SessionRepository {
    pub(crate) fn open(base: &Path) -> Result<Self> {
        std::fs::create_dir_all(base)
            .with_context(|| format!("failed to create session store root {}", base.display()))?;
        let repository = Self {
            database_path: base.join("sessions.sqlite3"),
        };
        repository.with_connection(initialize_schema)?;
        Ok(repository)
    }

    pub(crate) fn is_imported(&self) -> Result<bool> {
        self.with_connection(|connection| {
            Ok(connection
                .query_row(
                    "SELECT 1 FROM session_store_migrations WHERE migration_name = ?1",
                    [JSON_IMPORT_MIGRATION],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
    }

    pub(crate) fn import_legacy(&self, state: LegacyImportState) -> Result<bool> {
        self.with_connection(|connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let already_imported = transaction
                .query_row(
                    "SELECT 1 FROM session_store_migrations WHERE migration_name = ?1",
                    [JSON_IMPORT_MIGRATION],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if already_imported {
                return Ok(false);
            }

            // This row is committed with all imported records below. It must be
            // visible to the source ledger's foreign key inside the transaction,
            // but cannot survive a failed import because the transaction rolls
            // back as a unit.
            transaction.execute(
                "INSERT INTO session_store_migrations (migration_name, completed_at_ms) VALUES (?1, ?2)",
                params![JSON_IMPORT_MIGRATION, now_ms()],
            )?;

            for session in state.sessions.values() {
                replace_session(&transaction, session)?;
                let mut metadata = state
                    .metadata
                    .get(&session.id)
                    .cloned()
                    .unwrap_or_default();
                let snapshots = std::mem::take(&mut metadata.snapshots);
                let pre_revert = metadata.pre_revert.take();
                upsert_metadata(&transaction, &session.id, &metadata)?;
                for snapshot in snapshots.into_iter().take(MAX_SESSION_SNAPSHOTS) {
                    let override_messages = if snapshot_matches_session_prefix(&snapshot, session) {
                        None
                    } else {
                        Some(snapshot.as_slice())
                    };
                    insert_snapshot(
                        &transaction,
                        &session.id,
                        snapshot.len(),
                        override_messages,
                    )?;
                }
                if let Some(messages) = pre_revert {
                    upsert_revert_stash(&transaction, &session.id, &messages)?;
                }
            }

            for request in state.questions.values() {
                upsert_question(&transaction, request)?;
            }

            for source in state.sources {
                transaction.execute(
                    "INSERT INTO session_store_migration_sources (migration_name, source_path, source_digest, imported_at_ms)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        JSON_IMPORT_MIGRATION,
                        source.path.to_string_lossy(),
                        source.digest,
                        now_ms(),
                    ],
                )?;
            }
            transaction.commit()?;
            Ok(true)
        })
    }

    pub(crate) fn list_sessions(&self, workspace_root: Option<&str>) -> Result<Vec<Session>> {
        self.with_connection(|connection| {
            let ids = session_ids(connection, workspace_root)?;
            ids.into_iter()
                .map(|id| load_session(connection, &id)?.context("session disappeared during list"))
                .collect()
        })
    }

    pub(crate) fn list_summaries(&self, workspace_root: Option<&str>) -> Result<Vec<Session>> {
        self.with_connection(|connection| {
            let mut statement = match workspace_root {
                Some(_) => connection.prepare(
                    "SELECT session_json FROM session_records
                     WHERE workspace_root = ?1 OR directory = ?1
                     ORDER BY updated_at_ms DESC, session_id DESC",
                )?,
                None => connection.prepare(
                    "SELECT session_json FROM session_records
                     ORDER BY updated_at_ms DESC, session_id DESC",
                )?,
            };
            let mut rows = match workspace_root {
                Some(root) => statement.query([root])?,
                None => statement.query([])?,
            };
            let mut sessions = Vec::new();
            while let Some(row) = rows.next()? {
                let mut session: Session = serde_json::from_str(&row.get::<_, String>(0)?)
                    .context("failed to decode stored session header")?;
                session.messages.clear();
                sessions.push(session);
            }
            Ok(sessions)
        })
    }

    pub(crate) fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        self.with_connection(|connection| load_session(connection, session_id))
    }

    pub(crate) fn save_session(&self, session: &Session) -> Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            replace_session(&transaction, session)?;
            ensure_metadata(&transaction, &session.id)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub(crate) fn delete_session(&self, session_id: &str) -> Result<bool> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            transaction.execute(
                "DELETE FROM session_question_requests WHERE session_id = ?1",
                [session_id],
            )?;
            let removed = transaction.execute(
                "DELETE FROM session_records WHERE session_id = ?1",
                [session_id],
            )?;
            transaction.commit()?;
            Ok(removed > 0)
        })
    }

    pub(crate) fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let next_ordinal: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(ordinal) + 1, 0) FROM session_messages WHERE session_id = ?1",
                [session_id],
                |row| row.get(0),
            )?;
            let exists = transaction
                .query_row(
                    "SELECT 1 FROM session_records WHERE session_id = ?1",
                    [session_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                anyhow::bail!("session not found for append_message");
            }
            transaction.execute(
                "INSERT INTO session_messages (session_id, ordinal, message_id, role, message_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    session_id,
                    next_ordinal,
                    message.id,
                    message_role_name(&message.role),
                    serde_json::to_string(message)?,
                ],
            )?;
            touch_session(&transaction, session_id)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub(crate) fn append_message_part(
        &self,
        session_id: &str,
        message_id: &str,
        part: &MessagePart,
    ) -> Result<()> {
        self.with_connection(|connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let exists = transaction
                .query_row(
                    "SELECT 1 FROM session_records WHERE session_id = ?1",
                    [session_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                anyhow::bail!("session not found for append_message_part");
            }
            let exact = transaction
                .query_row(
                    "SELECT ordinal, message_json FROM session_messages
                     WHERE session_id = ?1 AND message_id = ?2 ORDER BY ordinal ASC LIMIT 1",
                    params![session_id, message_id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            let row = match exact {
                Some(row) => Some(row),
                None => transaction
                    .query_row(
                        "SELECT ordinal, message_json FROM session_messages
                         WHERE session_id = ?1 AND role = 'user' ORDER BY ordinal DESC LIMIT 1",
                        [session_id],
                        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                    )
                    .optional()?,
            }
                .context("message not found for append_message_part")?;
            let (ordinal, raw_message) = row;
            let mut message: Message = serde_json::from_str(&raw_message)
                .context("failed to decode stored session message")?;
            reduce_message_parts(&mut message.parts, part.clone());
            transaction.execute(
                "UPDATE session_messages SET message_json = ?3 WHERE session_id = ?1 AND ordinal = ?2",
                params![session_id, ordinal, serde_json::to_string(&message)?],
            )?;
            touch_session(&transaction, session_id)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub(crate) fn fork_session(&self, source_id: &str) -> Result<Option<Session>> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let Some(mut child) = load_session(&transaction, source_id)? else {
                return Ok(None);
            };
            child.id = uuid::Uuid::new_v4().to_string();
            child.title = format!("{} (fork)", child.title);
            child.time.created = Utc::now();
            child.time.updated = child.time.created;
            child.slug = None;
            replace_session(&transaction, &child)?;
            let metadata = SessionMeta {
                parent_id: Some(source_id.to_string()),
                ..SessionMeta::default()
            };
            upsert_metadata(&transaction, &child.id, &metadata)?;
            insert_snapshot(&transaction, &child.id, child.messages.len(), None)?;
            transaction.commit()?;
            Ok(Some(child))
        })
    }

    pub(crate) fn revert_session(&self, session_id: &str) -> Result<bool> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let Some((snapshot_id, message_count, snapshot_json)) = transaction
                .query_row(
                    "SELECT snapshot_id, message_count, snapshot_json FROM session_snapshot_points
                     WHERE session_id = ?1 ORDER BY snapshot_id DESC LIMIT 1",
                    [session_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()?
            else {
                return Ok(false);
            };
            let messages = load_messages(&transaction, session_id)?;
            upsert_revert_stash(&transaction, session_id, &messages)?;
            transaction.execute(
                "DELETE FROM session_snapshot_points WHERE snapshot_id = ?1",
                [snapshot_id],
            )?;
            if let Some(snapshot_json) = snapshot_json {
                let snapshot: Vec<Message> = serde_json::from_str(&snapshot_json)
                    .context("failed to decode stored legacy revert snapshot")?;
                transaction.execute(
                    "DELETE FROM session_messages WHERE session_id = ?1",
                    [session_id],
                )?;
                insert_messages(&transaction, session_id, &snapshot)?;
            } else {
                transaction.execute(
                    "DELETE FROM session_messages WHERE session_id = ?1 AND ordinal >= ?2",
                    params![session_id, message_count],
                )?;
            }
            touch_session(&transaction, session_id)?;
            transaction.commit()?;
            Ok(true)
        })
    }

    pub(crate) fn unrevert_session(&self, session_id: &str) -> Result<bool> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let Some(raw_messages) = transaction
                .query_row(
                    "SELECT messages_json FROM session_revert_stashes WHERE session_id = ?1",
                    [session_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            else {
                return Ok(false);
            };
            let previous: Vec<Message> = serde_json::from_str(&raw_messages)
                .context("failed to decode stored revert state")?;
            let current_count: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM session_messages WHERE session_id = ?1",
                [session_id],
                |row| row.get(0),
            )?;
            insert_snapshot(&transaction, session_id, current_count as usize, None)?;
            transaction.execute(
                "DELETE FROM session_messages WHERE session_id = ?1",
                [session_id],
            )?;
            insert_messages(&transaction, session_id, &previous)?;
            transaction.execute(
                "DELETE FROM session_revert_stashes WHERE session_id = ?1",
                [session_id],
            )?;
            touch_session(&transaction, session_id)?;
            transaction.commit()?;
            Ok(true)
        })
    }

    pub(crate) fn set_shared(&self, session_id: &str, shared: bool) -> Result<Option<String>> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut metadata = load_metadata(&transaction, session_id)?;
            metadata.shared = shared;
            if shared && metadata.share_id.is_none() {
                metadata.share_id = Some(uuid::Uuid::new_v4().to_string());
            }
            if !shared {
                metadata.share_id = None;
            }
            let share_id = metadata.share_id.clone();
            upsert_metadata(&transaction, session_id, &metadata)?;
            transaction.commit()?;
            Ok(share_id)
        })
    }

    pub(crate) fn set_archived(&self, session_id: &str, archived: bool) -> Result<bool> {
        self.update_metadata(session_id, |metadata| metadata.archived = archived)
            .map(|_| true)
    }

    pub(crate) fn set_summary(&self, session_id: &str, summary: String) -> Result<bool> {
        self.update_metadata(session_id, |metadata| metadata.summary = Some(summary))
            .map(|_| true)
    }

    pub(crate) fn children(&self, parent_id: &str) -> Result<Vec<Session>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT session_id FROM session_metadata WHERE parent_id = ?1 ORDER BY session_id",
            )?;
            let ids = statement
                .query_map([parent_id], |row| row.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            ids.into_iter()
                .map(|id| load_session(connection, &id)?.context("child session disappeared"))
                .collect()
        })
    }

    pub(crate) fn session_status(&self, session_id: &str) -> Result<Option<Value>> {
        self.with_connection(|connection| {
            let exists = connection
                .query_row(
                    "SELECT 1 FROM session_records WHERE session_id = ?1",
                    [session_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                return Ok(None);
            }
            let metadata = load_metadata(connection, session_id)?;
            let snapshot_count: i64 = connection.query_row(
                "SELECT COUNT(*) FROM session_snapshot_points WHERE session_id = ?1",
                [session_id],
                |row| row.get(0),
            )?;
            Ok(Some(json!({
                "archived": metadata.archived,
                "shared": metadata.shared,
                "parentID": metadata.parent_id,
                "snapshotCount": snapshot_count
            })))
        })
    }

    pub(crate) fn session_diff(&self, session_id: &str) -> Result<Option<Value>> {
        self.with_connection(|connection| {
            let exists = connection
                .query_row(
                    "SELECT 1 FROM session_records WHERE session_id = ?1",
                    [session_id],
                    |_| Ok(()),
                )
                .optional()?
                .is_some();
            if !exists {
                return Ok(None);
            }
            let current: i64 = connection.query_row(
                "SELECT COUNT(*) FROM session_messages WHERE session_id = ?1",
                [session_id],
                |row| row.get(0),
            )?;
            let last_snapshot: i64 = connection
                .query_row(
                    "SELECT message_count FROM session_snapshot_points WHERE session_id = ?1
                     ORDER BY snapshot_id DESC LIMIT 1",
                    [session_id],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(0);
            Ok(Some(json!({
                "sessionID": session_id,
                "currentMessageCount": current,
                "lastSnapshotMessageCount": last_snapshot,
                "delta": current - last_snapshot
            })))
        })
    }

    pub(crate) fn set_todos(&self, session_id: &str, todos: Vec<Value>) -> Result<()> {
        self.update_metadata(session_id, |metadata| metadata.todos = todos)
    }

    pub(crate) fn get_todos(&self, session_id: &str) -> Result<Vec<Value>> {
        self.with_connection(|connection| Ok(load_metadata(connection, session_id)?.todos))
    }

    pub(crate) fn add_question(&self, request: &QuestionRequest) -> Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            upsert_question(&transaction, request)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub(crate) fn list_questions(&self) -> Result<Vec<QuestionRequest>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT request_json FROM session_question_requests ORDER BY created_at_ms, request_id",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.map(|row| {
                let raw = row?;
                serde_json::from_str(&raw).context("failed to decode stored question request")
            })
            .collect()
        })
    }

    pub(crate) fn remove_question(&self, request_id: &str) -> Result<bool> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let removed = transaction.execute(
                "DELETE FROM session_question_requests WHERE request_id = ?1",
                [request_id],
            )?;
            transaction.commit()?;
            Ok(removed > 0)
        })
    }

    pub(crate) fn attach_to_workspace(
        &self,
        session_id: &str,
        target_workspace: &str,
        reason_tag: &str,
        project_id: Option<String>,
    ) -> Result<Option<Session>> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let Some(mut session) = load_session(&transaction, session_id)? else {
                return Ok(None);
            };
            let previous_workspace = session
                .workspace_root
                .clone()
                .or_else(|| Some(session.directory.clone()));
            if session.origin_workspace_root.is_none() {
                session.origin_workspace_root = previous_workspace.clone();
            }
            session.attached_from_workspace = previous_workspace;
            session.attached_to_workspace = Some(target_workspace.to_string());
            session.attach_timestamp_ms = Some(now_ms());
            session.attach_reason = Some(reason_tag.trim().to_string());
            session.workspace_root = Some(target_workspace.to_string());
            session.project_id = project_id;
            session.directory = target_workspace.to_string();
            session.time.updated = Utc::now();
            update_session_header(&transaction, &session)?;
            transaction.commit()?;
            Ok(Some(session))
        })
    }

    pub(crate) fn repair_sessions<F>(&self, mut repair: F) -> Result<super::SessionRepairStats>
    where
        F: FnMut(&Session) -> Option<(Session, super::SessionRepairStats)>,
    {
        self.with_connection(|connection| {
            let ids = session_ids(connection, None)?;
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut totals = super::SessionRepairStats::default();
            for id in ids {
                let Some(session) = load_session(&transaction, &id)? else {
                    continue;
                };
                let Some((repaired, stats)) = repair(&session) else {
                    continue;
                };
                replace_session(&transaction, &repaired)?;
                totals.sessions_repaired += stats.sessions_repaired;
                totals.messages_recovered += stats.messages_recovered;
                totals.parts_recovered += stats.parts_recovered;
                totals.conflicts_merged += stats.conflicts_merged;
            }
            transaction.commit()?;
            Ok(totals)
        })
    }

    fn update_metadata<F>(&self, session_id: &str, change: F) -> Result<()>
    where
        F: FnOnce(&mut SessionMeta),
    {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut metadata = load_metadata(&transaction, session_id)?;
            change(&mut metadata);
            upsert_metadata(&transaction, session_id, &metadata)?;
            transaction.commit()?;
            Ok(())
        })
    }

    fn with_connection<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T>,
    ) -> Result<T> {
        let mut connection = Connection::open(&self.database_path).with_context(|| {
            format!(
                "failed to open session store {}",
                self.database_path.display()
            )
        })?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")?;
        operation(&mut connection)
    }
}

fn initialize_schema(connection: &mut Connection) -> Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS session_store_migrations (
             migration_name TEXT PRIMARY KEY,
             completed_at_ms INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS session_store_migration_sources (
             migration_name TEXT NOT NULL,
             source_path TEXT NOT NULL,
             source_digest TEXT,
             imported_at_ms INTEGER NOT NULL,
             PRIMARY KEY (migration_name, source_path),
             FOREIGN KEY (migration_name) REFERENCES session_store_migrations(migration_name)
         );
         CREATE TABLE IF NOT EXISTS session_records (
             session_id TEXT PRIMARY KEY,
             workspace_root TEXT,
             directory TEXT NOT NULL,
             updated_at_ms INTEGER NOT NULL,
             session_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS session_records_workspace_idx
             ON session_records(workspace_root, updated_at_ms DESC);
         CREATE TABLE IF NOT EXISTS session_messages (
             session_id TEXT NOT NULL,
             ordinal INTEGER NOT NULL,
             message_id TEXT NOT NULL,
             role TEXT NOT NULL,
             message_json TEXT NOT NULL,
             PRIMARY KEY (session_id, ordinal),
             FOREIGN KEY (session_id) REFERENCES session_records(session_id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS session_messages_lookup_idx
             ON session_messages(session_id, message_id, ordinal);
         CREATE TABLE IF NOT EXISTS session_metadata (
             session_id TEXT PRIMARY KEY,
             parent_id TEXT,
             metadata_json TEXT NOT NULL,
             FOREIGN KEY (session_id) REFERENCES session_records(session_id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS session_metadata_parent_idx ON session_metadata(parent_id);
         CREATE TABLE IF NOT EXISTS session_snapshot_points (
             snapshot_id INTEGER PRIMARY KEY AUTOINCREMENT,
             session_id TEXT NOT NULL,
             message_count INTEGER NOT NULL,
             snapshot_json TEXT,
             FOREIGN KEY (session_id) REFERENCES session_records(session_id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS session_snapshot_points_lookup_idx
             ON session_snapshot_points(session_id, snapshot_id DESC);
         CREATE TABLE IF NOT EXISTS session_revert_stashes (
             session_id TEXT PRIMARY KEY,
             messages_json TEXT NOT NULL,
             FOREIGN KEY (session_id) REFERENCES session_records(session_id) ON DELETE CASCADE
         );
         CREATE TABLE IF NOT EXISTS session_question_requests (
             request_id TEXT PRIMARY KEY,
             session_id TEXT NOT NULL,
             created_at_ms INTEGER NOT NULL,
             request_json TEXT NOT NULL
         );
         CREATE INDEX IF NOT EXISTS session_question_requests_session_idx
             ON session_question_requests(session_id, created_at_ms);",
    )?;
    Ok(())
}

fn session_ids(connection: &Connection, workspace_root: Option<&str>) -> Result<Vec<String>> {
    let mut statement = match workspace_root {
        Some(_) => connection.prepare(
            "SELECT session_id FROM session_records WHERE workspace_root = ?1 OR directory = ?1
             ORDER BY updated_at_ms DESC, session_id DESC",
        )?,
        None => connection.prepare(
            "SELECT session_id FROM session_records ORDER BY updated_at_ms DESC, session_id DESC",
        )?,
    };
    let mut rows = match workspace_root {
        Some(root) => statement.query([root])?,
        None => statement.query([])?,
    };
    let mut ids = Vec::new();
    while let Some(row) = rows.next()? {
        ids.push(row.get(0)?);
    }
    Ok(ids)
}

fn load_session(connection: &Connection, session_id: &str) -> Result<Option<Session>> {
    let Some(mut session) = load_session_header(connection, session_id)? else {
        return Ok(None);
    };
    session.messages = load_messages(connection, session_id)?;
    Ok(Some(session))
}

fn load_session_header(connection: &Connection, session_id: &str) -> Result<Option<Session>> {
    let header = connection
        .query_row(
            "SELECT session_json FROM session_records WHERE session_id = ?1",
            [session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(header) = header else {
        return Ok(None);
    };
    let mut session: Session =
        serde_json::from_str(&header).context("failed to decode stored session header")?;
    session.messages.clear();
    Ok(Some(session))
}

fn load_messages(connection: &Connection, session_id: &str) -> Result<Vec<Message>> {
    let mut statement = connection.prepare(
        "SELECT message_json FROM session_messages WHERE session_id = ?1 ORDER BY ordinal ASC",
    )?;
    let rows = statement.query_map([session_id], |row| row.get::<_, String>(0))?;
    rows.map(|row| {
        let raw = row?;
        serde_json::from_str(&raw).context("failed to decode stored session message")
    })
    .collect()
}

fn replace_session(transaction: &Transaction<'_>, session: &Session) -> Result<()> {
    update_session_header(transaction, session)?;
    transaction.execute(
        "DELETE FROM session_messages WHERE session_id = ?1",
        [&session.id],
    )?;
    insert_messages(transaction, &session.id, &session.messages)
}

fn update_session_header(transaction: &Transaction<'_>, session: &Session) -> Result<()> {
    let mut header = session.clone();
    header.messages.clear();
    transaction.execute(
        "INSERT INTO session_records (session_id, workspace_root, directory, updated_at_ms, session_json)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(session_id) DO UPDATE SET
             workspace_root = excluded.workspace_root,
             directory = excluded.directory,
             updated_at_ms = excluded.updated_at_ms,
             session_json = excluded.session_json",
        params![
            session.id,
            session.workspace_root,
            session.directory,
            session.time.updated.timestamp_millis(),
            serde_json::to_string(&header)?,
        ],
    )?;
    Ok(())
}

fn insert_messages(
    transaction: &Transaction<'_>,
    session_id: &str,
    messages: &[Message],
) -> Result<()> {
    for (ordinal, message) in messages.iter().enumerate() {
        transaction.execute(
            "INSERT INTO session_messages (session_id, ordinal, message_id, role, message_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                session_id,
                ordinal as i64,
                message.id,
                message_role_name(&message.role),
                serde_json::to_string(message)?,
            ],
        )?;
    }
    Ok(())
}

fn touch_session(transaction: &Transaction<'_>, session_id: &str) -> Result<()> {
    let Some(mut session) = load_session_header(transaction, session_id)? else {
        anyhow::bail!("session not found");
    };
    session.time.updated = Utc::now();
    update_session_header(transaction, &session)
}

fn ensure_metadata(transaction: &Transaction<'_>, session_id: &str) -> Result<()> {
    let exists = transaction
        .query_row(
            "SELECT 1 FROM session_metadata WHERE session_id = ?1",
            [session_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        upsert_metadata(transaction, session_id, &SessionMeta::default())?;
    }
    Ok(())
}

fn load_metadata(connection: &Connection, session_id: &str) -> Result<SessionMeta> {
    let raw = connection
        .query_row(
            "SELECT metadata_json FROM session_metadata WHERE session_id = ?1",
            [session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    raw.map(|raw| serde_json::from_str(&raw).context("failed to decode stored session metadata"))
        .transpose()
        .map(|metadata| metadata.unwrap_or_default())
}

fn upsert_metadata(
    transaction: &Transaction<'_>,
    session_id: &str,
    metadata: &SessionMeta,
) -> Result<()> {
    let mut compact = metadata.clone();
    compact.snapshots.clear();
    compact.pre_revert = None;
    transaction.execute(
        "INSERT INTO session_metadata (session_id, parent_id, metadata_json) VALUES (?1, ?2, ?3)
         ON CONFLICT(session_id) DO UPDATE SET parent_id = excluded.parent_id, metadata_json = excluded.metadata_json",
        params![
            session_id,
            compact.parent_id,
            serde_json::to_string(&compact)?,
        ],
    )?;
    Ok(())
}

fn insert_snapshot(
    transaction: &Transaction<'_>,
    session_id: &str,
    message_count: usize,
    snapshot_override: Option<&[Message]>,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO session_snapshot_points (session_id, message_count, snapshot_json)
         VALUES (?1, ?2, ?3)",
        params![
            session_id,
            message_count as i64,
            snapshot_override.map(serde_json::to_string).transpose()?,
        ],
    )?;
    transaction.execute(
        "DELETE FROM session_snapshot_points
         WHERE session_id = ?1 AND snapshot_id NOT IN (
             SELECT snapshot_id FROM session_snapshot_points
             WHERE session_id = ?1 ORDER BY snapshot_id DESC LIMIT ?2
         )",
        params![session_id, MAX_SESSION_SNAPSHOTS as i64],
    )?;
    Ok(())
}

fn snapshot_matches_session_prefix(snapshot: &[Message], session: &Session) -> bool {
    if snapshot.len() > session.messages.len() {
        return false;
    }
    let current_prefix = &session.messages[..snapshot.len()];
    serde_json::to_vec(snapshot).ok() == serde_json::to_vec(current_prefix).ok()
}

fn upsert_revert_stash(
    transaction: &Transaction<'_>,
    session_id: &str,
    messages: &[Message],
) -> Result<()> {
    transaction.execute(
        "INSERT INTO session_revert_stashes (session_id, messages_json) VALUES (?1, ?2)
         ON CONFLICT(session_id) DO UPDATE SET messages_json = excluded.messages_json",
        params![session_id, serde_json::to_string(messages)?],
    )?;
    Ok(())
}

fn upsert_question(transaction: &Transaction<'_>, request: &QuestionRequest) -> Result<()> {
    transaction.execute(
        "INSERT INTO session_question_requests (request_id, session_id, created_at_ms, request_json)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(request_id) DO UPDATE SET session_id = excluded.session_id, request_json = excluded.request_json",
        params![
            request.id,
            request.session_id,
            now_ms(),
            serde_json::to_string(request)?,
        ],
    )?;
    Ok(())
}

fn message_role_name(role: &MessageRole) -> &'static str {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::System => "system",
        MessageRole::Tool => "tool",
    }
}

fn now_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}
