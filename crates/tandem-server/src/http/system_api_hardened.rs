// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::{
    extract::{Extension, Path, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use ignore::WalkBuilder;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    time::{Duration, Instant},
};
use tandem_types::{TenantContext, VerifiedTenantContext};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use super::sessions_actor_scope::ensure_same_session_actor;
use super::system_api::{
    self, FileContentQuery, FileListQuery, FindFileQuery, FindTextQuery, LspQuery, PathInfoQuery,
    PtyUpdateInput,
};
use crate::{
    action_authorization::{
        authorize_host_effect, CanonicalHostResource, HostAction, HostAuthorizationError,
        HostEffectRequest,
    },
    AppState,
};

const MAX_SEARCH_RESULTS: usize = 200;
const MAX_SEARCH_FILES: usize = 10_000;
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;
const MAX_SEARCH_DEPTH: usize = 32;
const MAX_FILE_CONTENT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_COMMAND_OUTPUT_BYTES: u64 = 1024 * 1024;
const SEARCH_DEADLINE: Duration = Duration::from_secs(2);
const COMMAND_DEADLINE: Duration = Duration::from_secs(15);

#[derive(Debug, Deserialize)]
pub(super) struct CommandRunInput {
    id: String,
}

fn require_local(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> Result<(), StatusCode> {
    super::host_authority::require_loopback_local_operator(state, tenant, verified)
}
fn authorization_status(error: HostAuthorizationError) -> StatusCode {
    match error {
        HostAuthorizationError::AuditPersistenceFailed => StatusCode::INTERNAL_SERVER_ERROR,
        HostAuthorizationError::InvalidEffectArguments => StatusCode::BAD_REQUEST,
        _ => StatusCode::FORBIDDEN,
    }
}

pub(super) async fn find_text(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    Query(query): Query<FindTextQuery>,
) -> Result<Json<Value>, StatusCode> {
    let (resource, workspace_root) = resolve_workspace_resource(
        &state,
        &tenant,
        verified.as_deref(),
        query.resource_id.as_deref(),
    )
    .await?;
    let limit = query.limit.unwrap_or(100).clamp(1, MAX_SEARCH_RESULTS);
    let effect = HostEffectRequest::new(
        HostAction::FileSearch,
        resource,
        json!({
            "operation": "find_text",
            "path": query.path.as_deref().unwrap_or("."),
            "pattern": &query.pattern,
            "result_limit": limit,
            "file_limit": MAX_SEARCH_FILES,
            "file_byte_limit": MAX_SEARCH_FILE_BYTES,
            "depth_limit": MAX_SEARCH_DEPTH,
            "deadline_ms": SEARCH_DEADLINE.as_millis(),
        }),
    );
    let grant = authorize_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality.is_direct_loopback(),
        &effect,
    )
    .await
    .map_err(authorization_status)?;
    let root = resolve_workspace_path(&workspace_root, query.path.as_deref(), true).await?;
    let regex = Regex::new(&query.pattern).map_err(|_| StatusCode::BAD_REQUEST)?;
    grant
        .revalidate(&state, &effect)
        .map_err(authorization_status)?;
    let matches = tokio::task::spawn_blocking(move || {
        search_text_bounded(&workspace_root, &root, &regex, limit)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!(matches)))
}

pub(super) async fn find_file(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    Query(query): Query<FindFileQuery>,
) -> Result<Json<Value>, StatusCode> {
    let (resource, workspace_root) = resolve_workspace_resource(
        &state,
        &tenant,
        verified.as_deref(),
        query.resource_id.as_deref(),
    )
    .await?;
    let limit = query.limit.unwrap_or(100).clamp(1, MAX_SEARCH_RESULTS);
    let effect = HostEffectRequest::new(
        HostAction::FileSearch,
        resource,
        json!({
            "operation": "find_file",
            "path": query.path.as_deref().unwrap_or("."),
            "query": &query.q,
            "result_limit": limit,
            "file_limit": MAX_SEARCH_FILES,
            "depth_limit": MAX_SEARCH_DEPTH,
            "deadline_ms": SEARCH_DEADLINE.as_millis(),
        }),
    );
    let grant = authorize_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality.is_direct_loopback(),
        &effect,
    )
    .await
    .map_err(authorization_status)?;
    let root = resolve_workspace_path(&workspace_root, query.path.as_deref(), true).await?;
    let needle = query.q.to_lowercase();
    grant
        .revalidate(&state, &effect)
        .map_err(authorization_status)?;
    let files = tokio::task::spawn_blocking(move || {
        search_files_bounded(&workspace_root, &root, &needle, limit)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!(files)))
}

pub(super) async fn find_symbol(
    state: State<AppState>,
    tenant: Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    locality: Extension<super::host_authority::RequestLocality>,
    query: Query<FindTextQuery>,
) -> Result<Json<Value>, StatusCode> {
    find_text(state, tenant, verified, locality, query).await
}

pub(super) async fn file_list(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    Query(query): Query<FileListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let (resource, workspace_root) = resolve_workspace_resource(
        &state,
        &tenant,
        verified.as_deref(),
        query.resource_id.as_deref(),
    )
    .await?;
    let limit = query.limit.unwrap_or(200).clamp(1, MAX_SEARCH_RESULTS);
    let effect = HostEffectRequest::new(
        HostAction::FileSearch,
        resource,
        json!({
            "operation": "file_list",
            "path": query.path.as_deref().unwrap_or("."),
            "result_limit": limit,
            "file_limit": MAX_SEARCH_FILES,
            "depth_limit": MAX_SEARCH_DEPTH,
            "deadline_ms": SEARCH_DEADLINE.as_millis(),
        }),
    );
    let grant = authorize_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality.is_direct_loopback(),
        &effect,
    )
    .await
    .map_err(authorization_status)?;
    let root = resolve_workspace_path(&workspace_root, query.path.as_deref(), true).await?;
    grant
        .revalidate(&state, &effect)
        .map_err(authorization_status)?;
    let files =
        tokio::task::spawn_blocking(move || list_files_bounded(&workspace_root, &root, limit))
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!(files)))
}

pub(super) async fn file_content(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    Query(query): Query<FileContentQuery>,
) -> Result<Json<Value>, StatusCode> {
    let (resource, workspace_root) = resolve_workspace_resource(
        &state,
        &tenant,
        verified.as_deref(),
        query.resource_id.as_deref(),
    )
    .await?;
    let effect = HostEffectRequest::new(
        HostAction::FileRead,
        resource,
        json!({
            "operation": "file_content",
            "path": &query.path,
            "byte_limit": MAX_FILE_CONTENT_BYTES,
        }),
    );
    let grant = authorize_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality.is_direct_loopback(),
        &effect,
    )
    .await
    .map_err(authorization_status)?;
    let target = resolve_workspace_path(&workspace_root, Some(&query.path), false).await?;
    grant
        .revalidate(&state, &effect)
        .map_err(authorization_status)?;
    let metadata = tokio::fs::metadata(&target)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !metadata.is_file() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if metadata.len() > MAX_FILE_CONTENT_BYTES {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let content = tokio::fs::read_to_string(&target)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(json!({"content": content})))
}

pub(super) async fn file_status(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    Ok(system_api::file_status().await)
}

pub(super) async fn vcs(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    Ok(system_api::vcs().await)
}

pub(super) async fn pty_list(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    Ok(system_api::pty_list(State(state)).await)
}

pub(super) async fn pty_create(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    system_api::pty_create(State(state)).await
}

pub(super) async fn pty_get(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    system_api::pty_get(State(state), Path(id)).await
}

pub(super) async fn pty_update(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(id): Path<String>,
    input: Json<PtyUpdateInput>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    system_api::pty_update(State(state), Path(id), input).await
}

pub(super) async fn pty_delete(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    system_api::pty_delete(State(state), Path(id)).await
}

pub(super) async fn pty_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(id): Path<String>,
) -> Response {
    if let Err(status) = require_local(&state, &tenant, verified.as_deref()) {
        return status.into_response();
    }
    system_api::pty_ws(ws, State(state), Path(id))
        .await
        .into_response()
}

pub(super) async fn lsp_status(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    query: Query<LspQuery>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    system_api::lsp_status(State(state), query).await
}

pub(super) async fn formatter_status(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    Ok(system_api::formatter_status().await)
}

pub(super) async fn command_list(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    Ok(Json(json!([
        {"id": "git-status", "title": "Git status"},
        {"id": "git-branch", "title": "Current Git branch"},
    ])))
}

pub(super) async fn run_command(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
    Json(input): Json<CommandRunInput>,
) -> Result<Json<Value>, StatusCode> {
    let request_id = system_api::request_id_from_headers(&headers);
    let started = Instant::now();
    let lookup_started = Instant::now();
    let session = state
        .storage
        .get_session(&session_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant, &session.tenant_context)?;
    let lookup_ms = lookup_started.elapsed().as_millis();

    let workspace = session
        .workspace_root
        .as_deref()
        .unwrap_or(session.directory.as_str());
    let cwd = tokio::fs::canonicalize(workspace)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if !tokio::fs::metadata(&cwd)
        .await
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    let (executable, args) = command_preset(&input.id).ok_or(StatusCode::BAD_REQUEST)?;
    let effect = HostEffectRequest::new(
        HostAction::CommandExecute,
        CanonicalHostResource::new(
            "session_workspace",
            session_id.clone(),
            session.tenant_context.clone(),
        ),
        json!({
            "command_id": input.id,
            "executable": executable,
            "args": args,
            "canonical_workspace": cwd.to_string_lossy(),
            "environment": ["GIT_CONFIG_NOSYSTEM", "GIT_CONFIG_GLOBAL", "GIT_OPTIONAL_LOCKS", "GIT_TERMINAL_PROMPT", "LC_ALL", "PATH"],
            "wall_time_ms": COMMAND_DEADLINE.as_millis(),
            "output_limit_bytes": MAX_COMMAND_OUTPUT_BYTES,
        }),
    );
    let grant = authorize_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality.is_direct_loopback(),
        &effect,
    )
    .await
    .map_err(authorization_status)?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .current_dir(&cwd)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", system_root);
    }

    grant
        .revalidate(&state, &effect)
        .map_err(authorization_status)?;
    let command_started = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let stderr = child
        .stderr
        .take()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let execution = tokio::time::timeout(COMMAND_DEADLINE, async {
        tokio::try_join!(read_limited(stdout), read_limited(stderr), child.wait(),)
    })
    .await;
    let ((stdout, stdout_truncated), (stderr, stderr_truncated), status) = match execution {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
        Err(_) => {
            let _ = child.kill().await;
            return Err(StatusCode::REQUEST_TIMEOUT);
        }
    };
    let command_ms = command_started.elapsed().as_millis();
    let elapsed_ms = started.elapsed().as_millis();
    tracing::info!(
        "session.command request_id={} session_id={} command_id={} ok={} elapsed_ms={} lookup_ms={} command_ms={}",
        request_id,
        session_id,
        input.id,
        status.success(),
        elapsed_ms,
        lookup_ms,
        command_ms
    );
    Ok(Json(json!({
        "ok": status.success(),
        "command_id": input.id,
        "stdout": stdout,
        "stderr": stderr,
        "stdout_truncated": stdout_truncated,
        "stderr_truncated": stderr_truncated,
    })))
}

pub(super) async fn path_info(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    query: Query<PathInfoQuery>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    Ok(system_api::path_info(State(state), query).await)
}

pub(super) async fn scheduler_metrics(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    require_local(&state, &tenant, verified.as_deref())?;
    Ok(system_api::scheduler_metrics(State(state)).await)
}

fn command_preset(id: &str) -> Option<(&'static str, &'static [&'static str])> {
    match id {
        "git-status" => Some((
            "git",
            &[
                "--no-pager",
                "-c",
                "core.fsmonitor=false",
                "status",
                "--short",
                "--untracked-files=no",
            ],
        )),
        "git-branch" => Some((
            "git",
            &[
                "--no-pager",
                "-c",
                "core.fsmonitor=false",
                "branch",
                "--show-current",
            ],
        )),
        _ => None,
    }
}

#[cfg(windows)]
fn null_device() -> &'static str {
    "NUL"
}

#[cfg(not(windows))]
fn null_device() -> &'static str {
    "/dev/null"
}

async fn read_limited<R>(reader: R) -> std::io::Result<(String, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::new();
    reader
        .take(MAX_COMMAND_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .await?;
    let truncated = bytes.len() as u64 > MAX_COMMAND_OUTPUT_BYTES;
    bytes.truncate(MAX_COMMAND_OUTPUT_BYTES as usize);
    Ok((String::from_utf8_lossy(&bytes).to_string(), truncated))
}

async fn resolve_workspace_resource(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    resource_id: Option<&str>,
) -> Result<(CanonicalHostResource, PathBuf), StatusCode> {
    if let Some(resource_id) = resource_id
        .map(str::trim)
        .filter(|resource_id| !resource_id.is_empty())
    {
        let session = state
            .storage
            .get_session(resource_id)
            .await
            .ok_or(StatusCode::NOT_FOUND)?;
        ensure_same_session_actor(tenant, &session.tenant_context)?;
        let workspace = session
            .workspace_root
            .as_deref()
            .unwrap_or(session.directory.as_str());
        let root = tokio::fs::canonicalize(workspace)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        if !tokio::fs::metadata(&root)
            .await
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        return Ok((
            CanonicalHostResource::new("session_workspace", resource_id, session.tenant_context),
            root,
        ));
    }
    if verified.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let root = canonical_workspace_root().await?;
    Ok((
        CanonicalHostResource::new("local_workspace", "local-workspace", tenant.clone()),
        root,
    ))
}
async fn canonical_workspace_root() -> Result<PathBuf, StatusCode> {
    let cwd = std::env::current_dir().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::canonicalize(cwd)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn resolve_workspace_path(
    workspace_root: &FsPath,
    requested: Option<&str>,
    require_directory: bool,
) -> Result<PathBuf, StatusCode> {
    let requested = requested.unwrap_or(".");
    let requested_path = PathBuf::from(requested);
    let candidate = if requested_path.is_absolute() {
        requested_path
    } else {
        workspace_root.join(requested_path)
    };
    let canonical = tokio::fs::canonicalize(candidate)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !canonical.starts_with(workspace_root) {
        tracing::warn!(requested_path = %requested, "blocked host filesystem path escape");
        return Err(StatusCode::FORBIDDEN);
    }
    if require_directory
        && !tokio::fs::metadata(&canonical)
            .await
            .map(|metadata| metadata.is_dir())
            .unwrap_or(false)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(canonical)
}

fn bounded_walker(root: &FsPath) -> ignore::Walk {
    WalkBuilder::new(root)
        .follow_links(false)
        .max_depth(Some(MAX_SEARCH_DEPTH))
        .build()
}

fn relative_path(workspace_root: &FsPath, path: &FsPath) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn search_text_bounded(
    workspace_root: &FsPath,
    root: &FsPath,
    regex: &Regex,
    limit: usize,
) -> Vec<Value> {
    let deadline = Instant::now() + SEARCH_DEADLINE;
    let mut visited_files = 0usize;
    let mut matches = Vec::new();
    for entry in bounded_walker(root).flatten() {
        if Instant::now() >= deadline || visited_files >= MAX_SEARCH_FILES || matches.len() >= limit
        {
            break;
        }
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        visited_files += 1;
        let path = entry.path();
        let Ok(metadata) = path.metadata() else {
            continue;
        };
        if metadata.len() > MAX_SEARCH_FILE_BYTES {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for (index, line) in content.lines().enumerate() {
            if regex.is_match(line) {
                matches.push(json!({
                    "path": relative_path(workspace_root, path),
                    "line": index + 1,
                    "text": truncate_line(line, 4096),
                }));
                if matches.len() >= limit {
                    break;
                }
            }
        }
    }
    matches
}

fn search_files_bounded(
    workspace_root: &FsPath,
    root: &FsPath,
    needle: &str,
    limit: usize,
) -> Vec<String> {
    list_files_bounded(workspace_root, root, MAX_SEARCH_FILES)
        .into_iter()
        .filter(|path| {
            FsPath::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_lowercase().contains(needle))
        })
        .take(limit)
        .collect()
}

fn list_files_bounded(workspace_root: &FsPath, root: &FsPath, limit: usize) -> Vec<String> {
    let deadline = Instant::now() + SEARCH_DEADLINE;
    let mut files = Vec::new();
    for entry in bounded_walker(root).flatten() {
        if Instant::now() >= deadline || files.len() >= limit.min(MAX_SEARCH_FILES) {
            break;
        }
        if entry.file_type().is_some_and(|kind| kind.is_file()) {
            files.push(relative_path(workspace_root, entry.path()));
        }
    }
    files
}

fn truncate_line(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &value[..boundary]
}

#[cfg(test)]
mod tests {
    use super::{
        command_preset, list_files_bounded, resolve_workspace_path, truncate_line, CommandRunInput,
    };
    use axum::http::StatusCode;

    #[test]
    fn command_presets_do_not_accept_executables_or_interpreters() {
        assert!(command_preset("git-status").is_some());
        assert!(command_preset("git-branch").is_some());
        let (executable, status_args) = command_preset("git-status").expect("git status preset");
        assert_eq!(executable, "git");
        assert!(status_args.contains(&"--no-pager"));
        assert!(status_args
            .windows(2)
            .any(|pair| pair == ["-c", "core.fsmonitor=false"]));
        for denied in ["git", "bash", "sh", "python", "python3", "cargo-check"] {
            assert!(command_preset(denied).is_none(), "{denied} must be denied");
        }
    }

    #[test]
    fn line_truncation_preserves_utf8_boundaries() {
        assert_eq!(truncate_line("aéz", 2), "a");
        assert_eq!(truncate_line("short", 20), "short");
    }

    #[test]
    fn legacy_command_payloads_cannot_supply_an_executable_or_arguments() {
        let payload = serde_json::json!({
            "command": "bash",
            "args": ["-c", "id"],
            "cwd": "/",
        });
        assert!(serde_json::from_value::<CommandRunInput>(payload).is_err());
    }

    #[tokio::test]
    async fn workspace_path_resolution_blocks_absolute_and_symlink_escapes() {
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        let outside = tempfile::tempdir().expect("outside tempdir");
        let root = tokio::fs::canonicalize(workspace.path())
            .await
            .expect("canonical workspace");
        let outside_path = outside.path().to_str().expect("outside utf8");
        assert_eq!(
            resolve_workspace_path(&root, Some(outside_path), false).await,
            Err(StatusCode::FORBIDDEN)
        );

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), workspace.path().join("escape"))
                .expect("create escape symlink");
            assert_eq!(
                resolve_workspace_path(&root, Some("escape"), false).await,
                Err(StatusCode::FORBIDDEN)
            );
        }
    }

    #[test]
    fn bounded_file_listing_honors_the_server_owned_result_limit() {
        let workspace = tempfile::tempdir().expect("workspace tempdir");
        for index in 0..5 {
            std::fs::write(workspace.path().join(format!("file-{index}.txt")), b"test")
                .expect("write fixture");
        }
        let root = workspace
            .path()
            .canonicalize()
            .expect("canonical workspace");
        let files = list_files_bounded(&root, &root, 2);
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|path| !path.starts_with('/')));
    }
}
