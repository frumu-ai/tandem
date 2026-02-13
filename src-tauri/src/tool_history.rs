use crate::error::{Result, TandemError};
use crate::sidecar::StreamEvent;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tandem_core::resolve_shared_paths;
use tandem_types::{MessagePart, Session};
use tauri::AppHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionRow {
    pub id: String,
    pub session_id: String,
    pub message_id: Option<String>,
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

fn app_memory_db_path(_app: &AppHandle) -> Result<PathBuf> {
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
    Ok(app_data_dir.join("memory.sqlite"))
}

pub fn app_memory_db_path_for_commands(app: &AppHandle) -> Result<PathBuf> {
    app_memory_db_path(app)
}

fn open_conn(app: &AppHandle) -> Result<Connection> {
    let db_path = app_memory_db_path(app)?;
    let conn =
        Connection::open(&db_path).map_err(|e| to_memory_error("open tool history db", e))?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS tool_executions (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL,
            message_id TEXT,
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
        "#,
    )
    .map_err(|e| to_memory_error("initialize tool history schema", e))?;

    Ok(conn)
}

fn normalize_call_id(part_id: &str, session_id: &str, message_id: &str, tool: &str) -> String {
    if !part_id.trim().is_empty() {
        part_id.to_string()
    } else {
        format!("{}:{}:{}", session_id, message_id, tool)
    }
}

fn to_json_text(value: &Value) -> Option<String> {
    serde_json::to_string(value).ok()
}

fn map_row_to_tool_execution(row: &rusqlite::Row<'_>) -> rusqlite::Result<ToolExecutionRow> {
    let started_at_i64: i64 = row.get(8)?;
    let ended_at_i64: Option<i64> = row.get(9)?;
    Ok(ToolExecutionRow {
        id: row.get(0)?,
        session_id: row.get(1)?,
        message_id: row.get(2)?,
        tool: row.get(3)?,
        status: row.get(4)?,
        args: row
            .get::<_, Option<String>>(5)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        result: row
            .get::<_, Option<String>>(6)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        error: row.get(7)?,
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
            let args_json = to_json_text(args);

            conn.execute(
                r#"
                INSERT INTO tool_executions (
                    id, session_id, message_id, tool, status, args_json,
                    started_at_ms, ended_at_ms, updated_at_ms
                ) VALUES (?1, ?2, ?3, ?4, 'running', ?5, ?6, NULL, ?7)
                ON CONFLICT(id) DO UPDATE SET
                    session_id = excluded.session_id,
                    message_id = excluded.message_id,
                    tool = excluded.tool,
                    status = 'running',
                    args_json = COALESCE(excluded.args_json, tool_executions.args_json),
                    started_at_ms = COALESCE(tool_executions.started_at_ms, excluded.started_at_ms),
                    updated_at_ms = excluded.updated_at_ms
                "#,
                params![id, session_id, message_id, tool, args_json, started_ms, now_ms],
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
        } => {
            let conn = open_conn(app)?;
            let now_ms = now_ms_i64()?;
            let id = normalize_call_id(part_id, session_id, message_id, tool);
            let status = if error.is_some() {
                "failed"
            } else {
                "completed"
            };
            let result_json = result.as_ref().and_then(to_json_text);

            conn.execute(
                r#"
                INSERT INTO tool_executions (
                    id, session_id, message_id, tool, status, result_json,
                    error_text, started_at_ms, ended_at_ms, updated_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(id) DO UPDATE SET
                    session_id = excluded.session_id,
                    message_id = excluded.message_id,
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
            id, session_id, message_id, tool, status, args_json, result_json,
            error_text, started_at_ms, ended_at_ms
        FROM tool_executions
        WHERE session_id = ?1
          AND COALESCE(ended_at_ms, started_at_ms) < ?2
        ORDER BY COALESCE(ended_at_ms, started_at_ms) DESC
        LIMIT ?3
    "#;
    let sql_no_before = r#"
        SELECT
            id, session_id, message_id, tool, status, args_json, result_json,
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
