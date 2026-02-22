use crate::error::{Result, TandemError};
use crate::sidecar::StreamEvent;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tandem_core::resolve_shared_paths;
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tandem_types::{MessagePart, Session};
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionRow {
    pub id: String,
    pub session_id: String,
    pub message_id: Option<String>,
    pub part_id: Option<String>,
    pub correlation_id: Option<String>,
    pub tool: String,
    pub status: String,
    pub args: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub started_at_ms: u64,
    pub ended_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolHistoryBackfillStats {
    pub sessions_scanned: u64,
    pub tool_rows_upserted: u64,
}

fn to_memory_error(context: &str, err: impl std::fmt::Display) -> TandemError {
    TandemError::Memory(format!("{}: {}", context, err))
}

fn to_i64(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| TandemError::Memory("timestamp overflow".to_string()))
}

fn now_ms_i64() -> Result<i64> {
    to_i64(crate::logs::now_ms())
}

fn app_tool_history_db_path(_app: &AppHandle) -> Result<PathBuf> {
    let app_data_dir = match resolve_shared_paths() {
        Ok(paths) => paths.canonical_root,
        Err(e) => dirs::data_dir().map(|d| d.join("tandem")).ok_or_else(|| {
            TandemError::InvalidConfig(format!(
                "Failed to resolve canonical shared app data dir: {}",
                e
            ))
        })?,
    };
    std::fs::create_dir_all(&app_data_dir)?;
    Ok(app_data_dir.join("tool_history.sqlite"))
}

pub fn app_memory_db_path_for_commands(app: &AppHandle) -> Result<PathBuf> {
    app_tool_history_db_path(app)
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS tool_executions (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            message_id TEXT,
            part_id TEXT,
            correlation_id TEXT,
            tool TEXT NOT NULL,
            status TEXT NOT NULL,
            args_json TEXT,
            result_json TEXT,
            error_text TEXT,
            started_at_ms INTEGER NOT NULL,
            ended_at_ms INTEGER,
            updated_at_ms INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_tool_exec_session_time
            ON tool_executions(session_id, started_at_ms DESC);
        CREATE INDEX IF NOT EXISTS idx_tool_exec_updated
            ON tool_executions(updated_at_ms DESC);
        CREATE INDEX IF NOT EXISTS idx_tool_exec_status
            ON tool_executions(status, started_at_ms DESC);
        "#,
    )
    .map_err(|e| to_memory_error("initialize tool history schema", e))?;

    // Existing installs may be on an older schema; ensure additive columns exist.
    let mut known_cols = std::collections::HashSet::new();
    let mut stmt = conn
        .prepare("PRAGMA table_info(tool_executions)")
        .map_err(|e| to_memory_error("read tool history schema", e))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| to_memory_error("query tool history schema", e))?;
    for row in rows {
        known_cols.insert(row.map_err(|e| to_memory_error("read schema row", e))?);
    }

    if !known_cols.contains("part_id") {
        conn.execute("ALTER TABLE tool_executions ADD COLUMN part_id TEXT", [])
            .map_err(|e| to_memory_error("add part_id column", e))?;
    }
    if !known_cols.contains("correlation_id") {
        conn.execute(
            "ALTER TABLE tool_executions ADD COLUMN correlation_id TEXT",
            [],
        )
        .map_err(|e| to_memory_error("add correlation_id column", e))?;
    }

    Ok(())
}

fn is_recoverable_sqlite_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("database disk image is malformed") || lower.contains("malformed")
}

fn backup_and_reset_tool_history_db(path: &std::path::Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }

    let parent = path
        .parent()
        .ok_or_else(|| TandemError::Memory("Invalid tool history db path".to_string()))?;
    let ts = crate::logs::now_ms();
    let backup_dir = parent
        .join("tool_history_backups")
        .join(format!("corrupt-{ts}"));
    std::fs::create_dir_all(&backup_dir)?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("tool_history.sqlite");
    let backup_path = backup_dir.join(file_name);
    let _ = std::fs::copy(path, &backup_path);
    let _ = std::fs::remove_file(path);

    Ok(Some(backup_path))
}

fn open_conn(app: &AppHandle) -> Result<Connection> {
    let db_path = app_tool_history_db_path(app)?;

    let open_and_init = || -> Result<Connection> {
        let conn =
            Connection::open(&db_path).map_err(|e| to_memory_error("open tool history db", e))?;
        ensure_schema(&conn)?;
        Ok(conn)
    };

    match open_and_init() {
        Ok(conn) => Ok(conn),
        Err(err) if is_recoverable_sqlite_error(&err.to_string()) => {
            emit_event(
                tracing::Level::WARN,
                ProcessKind::Desktop,
                ObservabilityEvent {
                    event: "tool_history.recovery.start",
                    component: "tool_history",
                    correlation_id: None,
                    session_id: None,
                    run_id: None,
                    message_id: None,
                    provider_id: None,
                    model_id: None,
                    status: Some("running"),
                    error_code: Some("TOOL_HISTORY_DB_MALFORMED"),
                    detail: Some("tool history recovery started"),
                },
            );
            tracing::warn!("tool_history recovery triggered: {}", err);
            let backup = match backup_and_reset_tool_history_db(&db_path) {
                Ok(backup) => backup,
                Err(recovery_err) => {
                    emit_event(
                        tracing::Level::ERROR,
                        ProcessKind::Desktop,
                        ObservabilityEvent {
                            event: "tool_history.recovery.fail",
                            component: "tool_history",
                            correlation_id: None,
                            session_id: None,
                            run_id: None,
                            message_id: None,
                            provider_id: None,
                            model_id: None,
                            status: Some("failed"),
                            error_code: Some("TOOL_HISTORY_BACKUP_FAILED"),
                            detail: Some("tool history backup/reset failed"),
                        },
                    );
                    return Err(recovery_err);
                }
            };
            if let Some(path) = backup {
                tracing::warn!("tool_history backup written: {}", path.display());
            }
            let conn = match open_and_init() {
                Ok(conn) => conn,
                Err(recovery_err) => {
                    emit_event(
                        tracing::Level::ERROR,
                        ProcessKind::Desktop,
                        ObservabilityEvent {
                            event: "tool_history.recovery.fail",
                            component: "tool_history",
                            correlation_id: None,
                            session_id: None,
                            run_id: None,
                            message_id: None,
                            provider_id: None,
                            model_id: None,
                            status: Some("failed"),
                            error_code: Some("TOOL_HISTORY_REOPEN_FAILED"),
                            detail: Some("tool history reopen after recovery failed"),
                        },
                    );
                    return Err(recovery_err);
                }
            };
            tracing::info!("tool_history.recovered");
            emit_event(
                tracing::Level::INFO,
                ProcessKind::Desktop,
                ObservabilityEvent {
                    event: "tool_history.recovery.success",
                    component: "tool_history",
                    correlation_id: None,
                    session_id: None,
                    run_id: None,
                    message_id: None,
                    provider_id: None,
                    model_id: None,
                    status: Some("ok"),
                    error_code: None,
                    detail: Some("tool history recovery completed"),
                },
            );
            Ok(conn)
        }
        Err(err) => Err(err),
    }
}

fn normalize_call_id(part_id: &str, session_id: &str, message_id: &str, tool: &str) -> String {
    if !part_id.trim().is_empty() {
        format!("{}:{}:{}:{}", session_id, message_id, part_id, tool)
    } else {
        format!("{}:{}:{}", session_id, message_id, tool)
    }
}

fn to_json_text(value: &Value) -> Option<String> {
    serde_json::to_string(value).ok()
}

fn map_row_to_tool_execution(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolExecutionRow> {
    let started_at_i64: i64 = row.get(10)?;
    let ended_at_i64: Option<i64> = row.get(11)?;
    Ok(ToolExecutionRow {
        id: row.get(0)?,
        session_id: row.get(1)?,
        message_id: row.get(2)?,
        part_id: row.get(3)?,
        correlation_id: row.get(4)?,
        tool: row.get(5)?,
        status: row.get(6)?,
        args: row
            .get::<_, Option<String>>(7)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        result: row
            .get::<_, Option<String>>(8)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        error: row.get(9)?,
        started_at_ms: u64::try_from(started_at_i64).unwrap_or_default(),
        ended_at_ms: ended_at_i64.and_then(|v| u64::try_from(v).ok()),
    })
}

pub fn record_stream_event(app: &AppHandle, event: &StreamEvent) -> Result<()> {
    match event {
        StreamEvent::ToolStart {
            session_id,
            message_id,
            part_id,
            tool,
            args,
        } => {
            let conn = open_conn(app)?;
            let now_ms = now_ms_i64()?;
            let started_ms = now_ms;
            let id = normalize_call_id(part_id, session_id, message_id, tool);
            let part_id_norm = if part_id.trim().is_empty() {
                None
            } else {
                Some(part_id.clone())
            };
            let correlation_id = format!("{}:{}:{}", session_id, message_id, id);
            let args_json = to_json_text(args);

            conn.execute(
                r#"
                INSERT INTO tool_executions (
                    id, session_id, message_id, part_id, correlation_id, tool, status, args_json,
                    started_at_ms, ended_at_ms, updated_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7, ?8, NULL, ?9)
                ON CONFLICT(id) DO UPDATE SET
                    session_id = excluded.session_id,
                    message_id = excluded.message_id,
                    part_id = COALESCE(excluded.part_id, tool_executions.part_id),
                    correlation_id = COALESCE(excluded.correlation_id, tool_executions.correlation_id),
                    tool = excluded.tool,
                    status = 'running',
                    args_json = COALESCE(excluded.args_json, tool_executions.args_json),
                    started_at_ms = COALESCE(tool_executions.started_at_ms, excluded.started_at_ms),
                    updated_at_ms = excluded.updated_at_ms
                "#,
                params![
                    id,
                    session_id,
                    message_id,
                    part_id_norm,
                    correlation_id,
                    tool,
                    args_json,
                    started_ms,
                    now_ms
                ],
            )
            .map_err(|e| to_memory_error("record tool_start", e))?;
            Ok(())
        }
        StreamEvent::ToolEnd {
            session_id,
            message_id,
            part_id,
            tool,
            result,
            error,
            ..
        } => {
            let conn = open_conn(app)?;
            let now_ms = now_ms_i64()?;
            let id = normalize_call_id(part_id, session_id, message_id, tool);
            let part_id_norm = if part_id.trim().is_empty() {
                None
            } else {
                Some(part_id.clone())
            };
            let correlation_id = format!("{}:{}:{}", session_id, message_id, id);
            let status = if error.is_some() {
                "failed"
            } else {
                "completed"
            };
            let result_json = result.as_ref().and_then(to_json_text);

            conn.execute(
                r#"
                INSERT INTO tool_executions (
                    id, session_id, message_id, part_id, correlation_id, tool, status, result_json,
                    error_text, started_at_ms, ended_at_ms, updated_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT(id) DO UPDATE SET
                    session_id = excluded.session_id,
                    message_id = excluded.message_id,
                    part_id = COALESCE(excluded.part_id, tool_executions.part_id),
                    correlation_id = COALESCE(excluded.correlation_id, tool_executions.correlation_id),
                    tool = excluded.tool,
                    status = excluded.status,
                    result_json = COALESCE(excluded.result_json, tool_executions.result_json),
                    error_text = COALESCE(excluded.error_text, tool_executions.error_text),
                    ended_at_ms = excluded.ended_at_ms,
                    updated_at_ms = excluded.updated_at_ms
                "#,
                params![
                    id,
                    session_id,
                    message_id,
                    part_id_norm,
                    correlation_id,
                    tool,
                    status,
                    result_json,
                    error.clone(),
                    now_ms,
                    now_ms,
                    now_ms
                ],
            )
            .map_err(|e| to_memory_error("record tool_end", e))?;
            Ok(())
        }
        StreamEvent::MemoryRetrieval {
            session_id,
            status,
            used,
            chunks_total,
            session_chunks,
            history_chunks,
            project_fact_chunks,
            latency_ms,
            query_hash,
            score_min,
            score_max,
            embedding_status,
            embedding_reason,
        } => {
            let conn = open_conn(app)?;
            let now_ms = now_ms_i64()?;
            let status_norm = status.as_deref().unwrap_or("unknown");
            let tool_status =
                if status_norm == "error_fallback" || status_norm == "degraded_disabled" {
                    "failed"
                } else {
                    "completed"
                };
            let id = format!("{}:memory-lookup:{}:{}", session_id, query_hash, now_ms);
            let correlation_id = format!("{}:memory-lookup:{}", session_id, query_hash);
            let args_json = serde_json::to_string(&json!({
                "status": status_norm,
                "used": used,
                "chunks_total": chunks_total,
                "session_chunks": session_chunks,
                "history_chunks": history_chunks,
                "project_fact_chunks": project_fact_chunks,
                "latency_ms": latency_ms,
                "query_hash": query_hash,
                "score_min": score_min,
                "score_max": score_max,
                "embedding_status": embedding_status,
                "embedding_reason": embedding_reason
            }))
            .ok();
            let result_json = serde_json::to_string(&json!({
                "summary": format!("lookup used={} chunks={} latency={}ms", used, chunks_total, latency_ms)
            }))
            .ok();
            let error_text = if tool_status == "failed" {
                Some(format!("lookup status={}", status_norm))
            } else {
                None
            };

            conn.execute(
                r#"
                INSERT INTO tool_executions (
                    id, session_id, message_id, part_id, correlation_id, tool, status, args_json, result_json,
                    error_text, started_at_ms, ended_at_ms, updated_at_ms
                ) VALUES (?1, ?2, NULL, NULL, ?3, 'memory.lookup', ?4, ?5, ?6, ?7, ?8, ?8, ?8)
                ON CONFLICT(id) DO UPDATE SET
                    session_id = excluded.session_id,
                    correlation_id = COALESCE(excluded.correlation_id, tool_executions.correlation_id),
                    tool = excluded.tool,
                    status = excluded.status,
                    args_json = COALESCE(excluded.args_json, tool_executions.args_json),
                    result_json = COALESCE(excluded.result_json, tool_executions.result_json),
                    error_text = COALESCE(excluded.error_text, tool_executions.error_text),
                    ended_at_ms = excluded.ended_at_ms,
                    updated_at_ms = excluded.updated_at_ms
                "#,
                params![
                    id,
                    session_id,
                    correlation_id,
                    tool_status,
                    args_json,
                    result_json,
                    error_text,
                    now_ms
                ],
            )
            .map_err(|e| to_memory_error("record memory_retrieval", e))?;
            tracing::info!(
                target: "tandem.memory",
                "memory_history_persist kind=lookup session_id={} status={} chunks_total={} latency_ms={}",
                session_id,
                status_norm,
                chunks_total,
                latency_ms
            );
            Ok(())
        }
        StreamEvent::MemoryStorage {
            session_id,
            message_id,
            role,
            session_chunks_stored,
            project_chunks_stored,
            status,
            error,
        } => {
            let conn = open_conn(app)?;
            let now_ms = now_ms_i64()?;
            let status_norm = status.as_deref().unwrap_or("unknown");
            let tool_status = if error.is_some() || status_norm == "error" {
                "failed"
            } else {
                "completed"
            };
            let id = if let Some(mid) = message_id.as_ref() {
                format!("{}:memory-store:{}:{}", session_id, role, mid)
            } else {
                format!("{}:memory-store:{}:{}", session_id, role, now_ms)
            };
            let correlation_id = if let Some(mid) = message_id.as_ref() {
                format!("{}:memory-store:{}:{}", session_id, role, mid)
            } else {
                format!("{}:memory-store:{}", session_id, role)
            };
            let args_json = serde_json::to_string(&json!({
                "role": role,
                "session_chunks_stored": session_chunks_stored,
                "project_chunks_stored": project_chunks_stored,
                "status": status_norm,
                "message_id": message_id
            }))
            .ok();
            let result_json = serde_json::to_string(&json!({
                "summary": format!("store role={} session={} project={}", role, session_chunks_stored, project_chunks_stored)
            }))
            .ok();

            conn.execute(
                r#"
                INSERT INTO tool_executions (
                    id, session_id, message_id, part_id, correlation_id, tool, status, args_json, result_json,
                    error_text, started_at_ms, ended_at_ms, updated_at_ms
                ) VALUES (?1, ?2, ?3, NULL, ?4, 'memory.store', ?5, ?6, ?7, ?8, ?9, ?9, ?9)
                ON CONFLICT(id) DO UPDATE SET
                    session_id = excluded.session_id,
                    message_id = COALESCE(excluded.message_id, tool_executions.message_id),
                    correlation_id = COALESCE(excluded.correlation_id, tool_executions.correlation_id),
                    tool = excluded.tool,
                    status = excluded.status,
                    args_json = COALESCE(excluded.args_json, tool_executions.args_json),
                    result_json = COALESCE(excluded.result_json, tool_executions.result_json),
                    error_text = COALESCE(excluded.error_text, tool_executions.error_text),
                    ended_at_ms = excluded.ended_at_ms,
                    updated_at_ms = excluded.updated_at_ms
                "#,
                params![
                    id,
                    session_id,
                    message_id,
                    correlation_id,
                    tool_status,
                    args_json,
                    result_json,
                    error.clone(),
                    now_ms
                ],
            )
            .map_err(|e| to_memory_error("record memory_storage", e))?;
            tracing::info!(
                target: "tandem.memory",
                "memory_history_persist kind=store session_id={} role={} status={} session_chunks_stored={} project_chunks_stored={} message_id={}",
                session_id,
                role,
                status_norm,
                session_chunks_stored,
                project_chunks_stored,
                message_id.as_deref().unwrap_or("none")
            );
            Ok(())
        }
        _ => Ok(()),
    }
}

pub fn list_tool_executions(
    app: &AppHandle,
    session_id: &str,
    limit: u32,
    before_ts_ms: Option<u64>,
) -> Result<Vec<ToolExecutionRow>> {
    let conn = open_conn(app)?;
    let limit = limit.clamp(1, 2000);
    let limit_i64 = i64::from(limit);

    let mut rows_out = Vec::new();
    let sql_with_before = r#"
        SELECT
            id, session_id, message_id, part_id, correlation_id, tool, status, args_json, result_json,
            error_text, started_at_ms, ended_at_ms
        FROM tool_executions
        WHERE session_id = ?1
          AND COALESCE(ended_at_ms, started_at_ms) < ?2
        ORDER BY COALESCE(ended_at_ms, started_at_ms) DESC
        LIMIT ?3
    "#;
    let sql_no_before = r#"
        SELECT
            id, session_id, message_id, part_id, correlation_id, tool, status, args_json, result_json,
            error_text, started_at_ms, ended_at_ms
        FROM tool_executions
        WHERE session_id = ?1
        ORDER BY COALESCE(ended_at_ms, started_at_ms) DESC
        LIMIT ?2
    "#;

    if let Some(before) = before_ts_ms {
        let before_i64 = to_i64(before)?;
        let mut stmt = conn
            .prepare(sql_with_before)
            .map_err(|e| to_memory_error("prepare tool history query", e))?;
        let mapped = stmt
            .query_map(
                params![session_id, before_i64, limit_i64],
                map_row_to_tool_execution,
            )
            .map_err(|e| to_memory_error("query tool history", e))?;
        for row in mapped {
            rows_out.push(row.map_err(|e| to_memory_error("read tool history row", e))?);
        }
    } else {
        let mut stmt = conn
            .prepare(sql_no_before)
            .map_err(|e| to_memory_error("prepare tool history query", e))?;
        let mapped = stmt
            .query_map(params![session_id, limit_i64], map_row_to_tool_execution)
            .map_err(|e| to_memory_error("query tool history", e))?;
        for row in mapped {
            rows_out.push(row.map_err(|e| to_memory_error("read tool history row", e))?);
        }
    }

    Ok(rows_out)
}

pub fn mark_running_tools_terminal(
    app: &AppHandle,
    session_id: Option<&str>,
    stale_after_ms: u64,
    reason: &str,
) -> Result<u64> {
    let conn = open_conn(app)?;
    let now_ms = now_ms_i64()?;
    let cutoff = if stale_after_ms == 0 {
        now_ms
    } else {
        now_ms.saturating_sub(to_i64(stale_after_ms)?)
    };

    let changed = if let Some(sid) = session_id {
        conn.execute(
            r#"
            UPDATE tool_executions
            SET
                status = 'failed',
                error_text = COALESCE(error_text, ?1),
                ended_at_ms = COALESCE(ended_at_ms, ?2),
                updated_at_ms = ?2
            WHERE session_id = ?3
              AND status = 'running'
              AND started_at_ms <= ?4
            "#,
            params![reason, now_ms, sid, cutoff],
        )
        .map_err(|e| to_memory_error("reconcile running tools by session", e))?
    } else {
        conn.execute(
            r#"
            UPDATE tool_executions
            SET
                status = 'failed',
                error_text = COALESCE(error_text, ?1),
                ended_at_ms = COALESCE(ended_at_ms, ?2),
                updated_at_ms = ?2
            WHERE status = 'running'
              AND started_at_ms <= ?3
            "#,
            params![reason, now_ms, cutoff],
        )
        .map_err(|e| to_memory_error("reconcile running tools", e))?
    };

    Ok(u64::try_from(changed).unwrap_or_default())
}

pub fn backfill_tool_executions_from_sessions(
    app: &AppHandle,
    sessions: &[Session],
) -> Result<ToolHistoryBackfillStats> {
    let mut stats = ToolHistoryBackfillStats::default();
    let mut conn = open_conn(app)?;
    let tx = conn
        .transaction()
        .map_err(|e| to_memory_error("start backfill transaction", e))?;
    let now_ms = now_ms_i64()?;

    for session in sessions {
        stats.sessions_scanned += 1;
        for message in &session.messages {
            let created_ms =
                u64::try_from(message.created_at.timestamp_millis()).unwrap_or_default();
            let created_ms_i64 = to_i64(created_ms)?;
            let mut ordinal: u64 = 0;
            for part in &message.parts {
                let MessagePart::ToolInvocation {
                    tool,
                    args,
                    result,
                    error,
                } = part
                else {
                    continue;
                };
                ordinal = ordinal.saturating_add(1);
                let id = format!("{}:{}:{}:{}", session.id, message.id, tool, ordinal);
                let status = if error.is_some() {
                    "failed"
                } else if result.is_some() {
                    "completed"
                } else {
                    "running"
                };
                let args_json = to_json_text(args);
                let result_json = result.as_ref().and_then(to_json_text);
                let ended_at = if status == "running" {
                    None
                } else {
                    Some(created_ms_i64)
                };

                tx.execute(
                    r#"
                    INSERT INTO tool_executions (
                        id, session_id, message_id, tool, status, args_json,
                        result_json, error_text, started_at_ms, ended_at_ms, updated_at_ms
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                    ON CONFLICT(id) DO UPDATE SET
                        session_id = excluded.session_id,
                        message_id = excluded.message_id,
                        tool = excluded.tool,
                        status = CASE
                            WHEN tool_executions.status = 'completed' THEN 'completed'
                            WHEN tool_executions.status = 'failed' THEN 'failed'
                            ELSE excluded.status
                        END,
                        args_json = COALESCE(tool_executions.args_json, excluded.args_json),
                        result_json = COALESCE(tool_executions.result_json, excluded.result_json),
                        error_text = COALESCE(tool_executions.error_text, excluded.error_text),
                        started_at_ms = COALESCE(tool_executions.started_at_ms, excluded.started_at_ms),
                        ended_at_ms = COALESCE(tool_executions.ended_at_ms, excluded.ended_at_ms),
                        updated_at_ms = excluded.updated_at_ms
                    "#,
                    params![
                        id,
                        session.id,
                        message.id,
                        tool,
                        status,
                        args_json,
                        result_json,
                        error.clone(),
                        created_ms_i64,
                        ended_at,
                        now_ms
                    ],
                )
                .map_err(|e| to_memory_error("backfill tool row", e))?;
                stats.tool_rows_upserted = stats.tool_rows_upserted.saturating_add(1);
            }
        }
    }

    tx.commit()
        .map_err(|e| to_memory_error("commit backfill transaction", e))?;
    Ok(stats)
}
