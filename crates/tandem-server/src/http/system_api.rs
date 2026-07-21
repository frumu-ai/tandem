// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket},
        Path, Query, State, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use ignore::WalkBuilder;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{path::PathBuf, time::Duration};
use tokio::process::Command;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Deserialize)]
pub(super) struct FindTextQuery {
    pub pattern: String,
    pub path: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FindFileQuery {
    pub q: String,
    pub path: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileListQuery {
    pub path: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct FileContentQuery {
    pub path: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct PtyUpdateInput {
    pub input: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct LspQuery {
    pub action: Option<String>,
    pub path: Option<String>,
    pub symbol: Option<String>,
    pub q: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ShellRunInput {
    pub command: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct PathInfoQuery {
    pub refresh: Option<bool>,
}

pub(super) async fn find_text(
    Query(query): Query<FindTextQuery>,
) -> Result<Json<Value>, StatusCode> {
    let root = query.path.unwrap_or_else(|| ".".to_string());
    let regex = Regex::new(&query.pattern).map_err(|_| StatusCode::BAD_REQUEST)?;
    let mut matches = Vec::new();
    let limit = query.limit.unwrap_or(100).max(1);

    for entry in WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if let Ok(content) = std::fs::read_to_string(path) {
            for (index, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    matches.push(json!({
                        "path": path.display().to_string(),
                        "line": index + 1,
                        "text": line
                    }));
                    if matches.len() >= limit {
                        return Ok(Json(json!(matches)));
                    }
                }
            }
        }
    }
    Ok(Json(json!(matches)))
}

pub(super) async fn find_file(Query(query): Query<FindFileQuery>) -> Json<Value> {
    let root = query.path.unwrap_or_else(|| ".".to_string());
    let needle = query.q.to_lowercase();
    let mut files = Vec::new();
    let limit = query.limit.unwrap_or(100).max(1);
    for entry in WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
        if name.to_lowercase().contains(&needle) {
            files.push(path.display().to_string());
            if files.len() >= limit {
                break;
            }
        }
    }
    Json(json!(files))
}

pub(super) async fn find_symbol(
    Query(query): Query<FindTextQuery>,
) -> Result<Json<Value>, StatusCode> {
    find_text(Query(query)).await
}

pub(super) async fn file_list(Query(query): Query<FileListQuery>) -> Json<Value> {
    let root = query.path.unwrap_or_else(|| ".".to_string());
    let mut files = Vec::new();
    let limit = query.limit.unwrap_or(200).max(1);
    for entry in WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }
        files.push(entry.path().display().to_string());
        if files.len() >= limit {
            break;
        }
    }
    Json(json!(files))
}

pub(super) async fn file_content(
    Query(query): Query<FileContentQuery>,
) -> Result<Json<Value>, StatusCode> {
    let query_path = query.path;
    let requested_path = PathBuf::from(&query_path);

    let canonical_cwd = std::env::current_dir().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let target_path = if requested_path.is_absolute() {
        requested_path
    } else {
        canonical_cwd.join(&requested_path)
    };

    let canonical_target = target_path
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;

    if !canonical_target.starts_with(&canonical_cwd) {
        tracing::warn!(
            "file_content path traversal attempt blocked: requested={} canonical={}",
            query_path,
            canonical_target.display()
        );
        return Err(StatusCode::FORBIDDEN);
    }

    let content = tokio::fs::read_to_string(&canonical_target)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(json!({"content": content})))
}

pub(super) async fn file_status() -> Json<Value> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .await
        .ok();
    let files = output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let status = line[0..2].trim().to_string();
            let path = line[3..].to_string();
            Some(json!({"status": status, "path": path}))
        })
        .collect::<Vec<_>>();
    Json(json!(files))
}

pub(super) async fn vcs() -> Json<Value> {
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let numstat_raw = Command::new("git")
        .args(["diff", "--numstat"])
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let numstat = numstat_raw
        .lines()
        .filter_map(|line| {
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.len() < 3 {
                return None;
            }
            Some(json!({
                "added": parts[0],
                "removed": parts[1],
                "path": parts[2]
            }))
        })
        .collect::<Vec<_>>();
    Json(json!({"branch": branch, "numstat": numstat}))
}

pub(super) async fn pty_list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.pty.list().await))
}

pub(super) async fn pty_create(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let id = state
        .pty
        .create()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true, "id": id})))
}

pub(super) async fn pty_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let snapshot = state.pty.snapshot(&id).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!(snapshot)))
}

pub(super) async fn pty_update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<PtyUpdateInput>,
) -> Result<Json<Value>, StatusCode> {
    if let Some(data) = input.input.as_ref() {
        let ok = state
            .pty
            .write(&id, data)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(Json(json!({"ok": ok})));
    }
    Ok(Json(json!({"ok": false, "error":"missing input"})))
}

pub(super) async fn pty_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .pty
        .kill(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": ok})))
}

pub(super) async fn pty_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| pty_ws_stream(socket, state, id))
}

async fn pty_ws_stream(mut socket: WebSocket, state: AppState, id: String) {
    let mut offset = 0usize;
    loop {
        let Some((chunk, next_offset, running)) = state.pty.read_since(&id, offset).await else {
            let _ = socket
                .send(WsMessage::Text("{\"closed\":true}".into()))
                .await;
            break;
        };
        if !chunk.is_empty() {
            let payload =
                json!({"id": id, "chunk": truncate_for_stream(&chunk, 4096), "running": running})
                    .to_string();
            if socket.send(WsMessage::Text(payload.into())).await.is_err() {
                break;
            }
        }
        offset = next_offset;
        if !running {
            let _ = socket
                .send(WsMessage::Text("{\"closed\":true}".into()))
                .await;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn truncate_for_stream(s: &str, limit: usize) -> &str {
    if s.len() <= limit {
        s
    } else {
        &s[..limit]
    }
}

pub(super) async fn lsp_status(
    State(state): State<AppState>,
    Query(query): Query<LspQuery>,
) -> Result<Json<Value>, StatusCode> {
    let action = query.action.as_deref().unwrap_or("status");
    match action {
        "status" => Ok(Json(json!({"ok": true, "backend": "heuristic-lsp"}))),
        "diagnostics" => {
            let path = query.path.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(json!(state.lsp.diagnostics(&path))))
        }
        "definition" => {
            let symbol = query.symbol.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(json!(state.lsp.goto_definition(&symbol))))
        }
        "references" => {
            let symbol = query.symbol.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(json!(state.lsp.references(&symbol))))
        }
        "hover" => {
            let symbol = query.symbol.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(json!({"text": state.lsp.hover(&symbol)})))
        }
        "symbols" => Ok(Json(json!(state.lsp.symbols(query.q.as_deref())))),
        "call_hierarchy" => {
            let symbol = query.symbol.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(state.lsp.call_hierarchy(&symbol)))
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

pub(super) async fn formatter_status() -> Json<Value> {
    Json(json!([]))
}

pub(crate) fn request_id_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-tandem-correlation-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string())
}

pub(super) async fn run_shell(Json(input): Json<ShellRunInput>) -> Result<Json<Value>, StatusCode> {
    tracing::warn!("run_shell endpoint called - this endpoint is disabled for security reasons");
    Err(StatusCode::FORBIDDEN)
}

pub(super) async fn path_info(
    State(state): State<AppState>,
    Query(query): Query<PathInfoQuery>,
) -> Json<Value> {
    let refresh = query.refresh.unwrap_or(false);
    let snapshot = if refresh {
        state.workspace_index.refresh().await
    } else {
        state.workspace_index.snapshot().await
    };
    Json(json!({
        "workspace": snapshot,
        "inProcessMode": state.in_process_mode.load(std::sync::atomic::Ordering::Relaxed)
    }))
}

pub(super) async fn scheduler_metrics(State(state): State<AppState>) -> Json<Value> {
    let scheduler = state.automation_scheduler.read().await;
    Json(json!(scheduler.metrics()))
}
