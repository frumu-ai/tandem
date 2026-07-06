use std::time::Instant;

use super::session_kb_grounding::{
    apply_strict_kb_grounding_after_run, policy_answer_question_tool,
    render_strict_kb_direct_answer, tool_allowlist_for_kb_grounding,
};
use super::*;
use crate::app::rate_limit::{
    channel_rate_limit_key_from_session_metadata, retry_after_duration, ChannelRateLimitKind,
};
use sha2::{Digest, Sha256};
use tandem_types::{RequestPrincipal, Session, ToolMode, VerifiedTenantContext};

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(super) enum SessionScope {
    Workspace,
    Global,
}

fn tenant_context_event_value(tenant_context: &TenantContext) -> Value {
    serde_json::to_value(tenant_context).unwrap_or_else(|_| json!(tenant_context))
}

fn with_tenant_context(mut properties: Value, tenant_context: &TenantContext) -> Value {
    if let Some(map) = properties.as_object_mut() {
        map.insert(
            "tenantContext".to_string(),
            tenant_context_event_value(tenant_context),
        );
    }
    properties
}

pub(super) fn publish_tenant_event(
    state: &AppState,
    tenant_context: &TenantContext,
    event_type: &str,
    properties: Value,
) {
    state.event_bus.publish(EngineEvent::new(
        event_type,
        with_tenant_context(properties, tenant_context),
    ));
}

fn tenant_actor_id(tenant_context: &TenantContext) -> Option<&str> {
    tenant_context
        .actor_id
        .as_deref()
        .map(str::trim)
        .filter(|actor_id| !actor_id.is_empty())
}

fn session_visible_to_actor(
    request_tenant: &TenantContext,
    session_tenant: &TenantContext,
) -> bool {
    if !tenant_matches(request_tenant, session_tenant) {
        return false;
    }
    if request_tenant.is_local_implicit() && session_tenant.is_local_implicit() {
        return true;
    }
    matches!(
        (
            tenant_actor_id(request_tenant),
            tenant_actor_id(session_tenant)
        ),
        (Some(request_actor), Some(session_actor)) if request_actor == session_actor
    )
}

fn ensure_same_session_actor(
    request_tenant: &TenantContext,
    session_tenant: &TenantContext,
) -> Result<(), StatusCode> {
    if session_visible_to_actor(request_tenant, session_tenant) {
        Ok(())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

fn mcp_namespace_segment_for_grounding(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "server".to_string()
    } else {
        cleaned.to_string()
    }
}

fn mcp_server_is_knowledgebase(server: &tandem_runtime::McpServer) -> bool {
    server.grounding_required
        || server.purpose.trim().eq_ignore_ascii_case("knowledgebase")
        || server.name.trim().eq_ignore_ascii_case("kb")
}

fn explicit_allowlist_patterns_for_mcp_server(
    allowlist: &[String],
    server_name: &str,
    strict_kb_grounding: bool,
) -> Vec<String> {
    let namespace = mcp_namespace_segment_for_grounding(server_name);
    let prefix = format!("mcp.{namespace}.");
    let wildcard = format!("mcp.{namespace}.*");
    let mut seen = std::collections::HashSet::new();
    let mut patterns = allowlist
        .iter()
        .map(|entry| entry.trim().to_ascii_lowercase())
        .filter(|entry| !entry.is_empty() && entry != "*")
        .filter(|entry| {
            entry == &wildcard || entry.starts_with(&prefix) || entry == &format!("mcp.{namespace}")
        })
        .filter(|entry| seen.insert(entry.clone()))
        .collect::<Vec<_>>();
    if patterns.is_empty()
        && strict_kb_grounding
        && allowlist.iter().any(|entry| entry.trim() == "*")
    {
        patterns.push(wildcard);
    }
    patterns
}

fn send_message_request_text(req: &SendMessageRequest) -> String {
    req.parts
        .iter()
        .map(|part| match part {
            MessagePartInput::Text { text } => text.clone(),
            MessagePartInput::File {
                mime,
                filename,
                url,
            } => format!(
                "[file mime={} name={} url={}]",
                mime,
                filename.clone().unwrap_or_else(|| "unknown".to_string()),
                url
            ),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn kb_grounding_should_skip_query(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    let social = [
        "hi",
        "hello",
        "hey",
        "thanks",
        "thank you",
        "ok",
        "okay",
        "cool",
        "nice",
        "yo",
        "good morning",
        "good afternoon",
        "good evening",
    ];
    lower.len() <= 32 && social.contains(&lower.as_str())
}

async fn derive_session_kb_grounding_policy(
    state: &AppState,
    req: &SendMessageRequest,
) -> Option<tandem_core::KnowledgebaseGroundingPolicy> {
    if kb_grounding_should_skip_query(&send_message_request_text(req)) {
        return None;
    }
    let allowlist = req.tool_allowlist.as_deref()?;
    if allowlist.is_empty() {
        return None;
    }
    let servers = state.mcp.list_public().await;
    let mut server_names = Vec::new();
    let mut tool_patterns = Vec::new();
    for server in servers.values() {
        if !server.enabled || !mcp_server_is_knowledgebase(server) {
            continue;
        }
        let patterns = explicit_allowlist_patterns_for_mcp_server(
            allowlist,
            &server.name,
            req.strict_kb_grounding.unwrap_or(false),
        );
        if patterns.is_empty() {
            continue;
        }
        server_names.push(server.name.clone());
        tool_patterns.extend(patterns);
    }
    if tool_patterns.is_empty() {
        return None;
    }
    Some(tandem_core::KnowledgebaseGroundingPolicy {
        required: true,
        strict: req.strict_kb_grounding.unwrap_or(false),
        server_names,
        tool_patterns,
    })
}

fn request_is_text_only(req: &SendMessageRequest) -> bool {
    req.parts
        .iter()
        .all(|part| matches!(part, MessagePartInput::Text { .. }))
}

pub(super) async fn create_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<WireSession>, HttpError> {
    let requested_permission_rules = req.permission.clone();
    let mut session = Session::new(req.title, req.directory);
    session.tenant_context = tenant_context.clone();
    session.verified_tenant_context = verified_tenant_context.map(|Extension(verified)| verified);
    session.project_id = req.project_id.clone();
    let workspace_from_runtime = {
        let snapshot = state.workspace_index.snapshot().await;
        tandem_core::normalize_workspace_path(&snapshot.root)
    };
    let workspace = req
        .workspace_root
        .as_deref()
        .and_then(tandem_core::normalize_workspace_path)
        .or_else(|| tandem_core::normalize_workspace_path(&session.directory))
        .or(workspace_from_runtime);
    if let Some(workspace) = workspace {
        session.workspace_root = Some(workspace.clone());
        session.project_id = tandem_core::workspace_project_id(&workspace);
        if session.directory.trim() == "." || session.directory.trim().is_empty() {
            session.directory = workspace;
        }
    }
    session.environment = Some(state.host_runtime_context());
    session.model = req.model;
    session.provider = req.provider;
    session.sampling = req.sampling;
    apply_created_session_source(&mut session, req.source_kind, req.source_metadata);
    session.pinned_workspace_id = req
        .pinned_workspace_id
        .as_deref()
        .and_then(tandem_core::normalize_workspace_path)
        .or_else(|| {
            if session.source_kind.as_deref() == Some("channel") {
                session.workspace_root.clone()
            } else {
                None
            }
        });
    state
        .storage
        .save_session(session.clone())
        .await
        .map_err(|error| {
            tracing::error!(error = %error, session_id = %session.id, "failed to save created session");
            persistence_error(format!("Failed to save session: {error}"))
        })?;
    apply_session_permission_rules(&state, requested_permission_rules).await;
    publish_tenant_event(
        &state,
        &session.tenant_context,
        "session.created",
        json!({"sessionID": session.id, "projectID": session.project_id}),
    );
    Ok(Json(session.into()))
}

pub(super) async fn apply_session_permission_rules(
    state: &AppState,
    rules: Option<Vec<serde_json::Value>>,
) {
    let Some(rules) = rules else {
        return;
    };
    for raw in rules {
        let Some((permission, pattern, action)) = parse_permission_rule_input(&raw) else {
            continue;
        };
        let _ = state
            .permissions
            .add_rule(permission, pattern, action)
            .await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchivedExchangeCandidate {
    user_message_id: String,
    assistant_message_id: String,
    user_text: String,
    assistant_text: String,
}

fn message_text(message: &Message) -> Option<String> {
    let text = message
        .parts
        .iter()
        .filter_map(|part| match part {
            MessagePart::Text { text } => Some(text.trim()),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn should_archive_user_text(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && !trimmed.starts_with('/')
}

fn should_archive_assistant_text(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && !trimmed.starts_with("ENGINE_ERROR:")
}

fn latest_archived_exchange_candidate(session: &Session) -> Option<ArchivedExchangeCandidate> {
    let mut latest_assistant: Option<(usize, &Message, String)> = None;
    for (idx, message) in session.messages.iter().enumerate().rev() {
        if !matches!(message.role, MessageRole::Assistant) {
            continue;
        }
        let Some(text) = message_text(message) else {
            continue;
        };
        if !should_archive_assistant_text(&text) {
            continue;
        }
        latest_assistant = Some((idx, message, text));
        break;
    }

    let (assistant_idx, assistant_message, assistant_text) = latest_assistant?;
    for message in session.messages[..assistant_idx].iter().rev() {
        if !matches!(message.role, MessageRole::User) {
            continue;
        }
        let Some(user_text) = message_text(message) else {
            continue;
        };
        if !should_archive_user_text(&user_text) {
            continue;
        }
        return Some(ArchivedExchangeCandidate {
            user_message_id: message.id.clone(),
            assistant_message_id: assistant_message.id.clone(),
            user_text,
            assistant_text,
        });
    }

    None
}

fn archive_source_hash(session_id: &str, candidate: &ArchivedExchangeCandidate) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update(b":");
    hasher.update(candidate.user_message_id.as_bytes());
    hasher.update(b":");
    hasher.update(candidate.assistant_message_id.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn archived_exchange_content(session: &Session, candidate: &ArchivedExchangeCandidate) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Session title: {}", session.title));
    if let Some(workspace_root) = session.workspace_root.as_deref() {
        lines.push(format!("Workspace: {}", workspace_root));
    }
    if let Some(project_id) = session.project_id.as_deref() {
        lines.push(format!("Project ID: {}", project_id));
    }
    lines.push(format!("User message ID: {}", candidate.user_message_id));
    lines.push(format!(
        "Assistant message ID: {}",
        candidate.assistant_message_id
    ));
    lines.push(String::new());
    lines.push("User:".to_string());
    lines.push(candidate.user_text.clone());
    lines.push(String::new());
    lines.push("Assistant:".to_string());
    lines.push(candidate.assistant_text.clone());
    lines.join("\n")
}

pub(super) async fn archive_session_exchange_to_global_memory(state: AppState, session_id: String) {
    let Some(session) = state.storage.get_session(&session_id).await else {
        return;
    };
    let title = session.title.to_ascii_lowercase();
    if !(title.starts_with("telegram ")
        || title.starts_with("telegram —")
        || title.starts_with("discord ")
        || title.starts_with("discord —")
        || title.starts_with("slack ")
        || title.starts_with("slack —"))
    {
        return;
    }
    let Some(candidate) = latest_archived_exchange_candidate(&session) else {
        return;
    };

    if let Some(parent) = state.memory_db_path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            tracing::warn!(
                "global chat exchange archival could not create memory db parent for session {}: {}",
                session_id,
                err
            );
            return;
        }
    }
    let Ok(manager) = tandem_memory::manager::MemoryManager::new(&state.memory_db_path).await
    else {
        return;
    };

    let source_hash = archive_source_hash(&session_id, &candidate);
    match manager
        .db()
        .global_chunk_exists_by_source_hash_for_tenant(
            &source_hash,
            &tandem_memory::types::MemoryTenantScope {
                org_id: session.tenant_context.org_id.clone(),
                workspace_id: session.tenant_context.workspace_id.clone(),
                deployment_id: session.tenant_context.deployment_id.clone(),
            },
        )
        .await
    {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            tracing::warn!(
                "global memory dedupe check failed for session {}: {}",
                session_id,
                err
            );
            return;
        }
    }

    let metadata = json!({
        "source_kind": "chat_exchange",
        "session_id": session_id,
        "project_id": session.project_id,
        "workspace_root": session.workspace_root,
        "user_message_id": candidate.user_message_id,
        "assistant_message_id": candidate.assistant_message_id,
    });

    let request = tandem_memory::types::StoreMessageRequest {
        content: archived_exchange_content(&session, &candidate),
        tier: tandem_memory::types::MemoryTier::Global,
        session_id: Some(session.id.clone()),
        project_id: session.project_id.clone(),
        source: "chat_exchange".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: Some(source_hash),
        tenant_scope: tandem_memory::types::MemoryTenantScope {
            org_id: session.tenant_context.org_id.clone(),
            workspace_id: session.tenant_context.workspace_id.clone(),
            deployment_id: session.tenant_context.deployment_id.clone(),
        },
        // Archived chat exchanges are per-user memory: stamp the session actor
        // so governed reads restrict them to their owner (TAN-607). Local
        // sessions without an actor stay shared, matching prior behavior.
        subject: session.tenant_context.actor_id.clone(),
        metadata: Some(metadata),
    };

    if let Err(err) = manager.store_message(request).await {
        tracing::warn!(
            "global chat exchange archival failed for session {}: {}",
            session.id,
            err
        );
    }
}

pub(super) fn parse_permission_rule_input(
    raw: &serde_json::Value,
) -> Option<(String, String, tandem_core::PermissionAction)> {
    let obj = raw.as_object()?;
    let permission = obj.get("permission")?.as_str()?.trim().to_string();
    if permission.is_empty() {
        return None;
    }
    let pattern = obj
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(permission.as_str())
        .to_string();
    let action = obj.get("action").and_then(|v| v.as_str())?;
    let action = match action.trim().to_ascii_lowercase().as_str() {
        "allow" | "always" => tandem_core::PermissionAction::Allow,
        "ask" | "once" => tandem_core::PermissionAction::Ask,
        "deny" | "reject" => tandem_core::PermissionAction::Deny,
        _ => return None,
    };
    Some((permission, pattern, action))
}

pub(super) async fn list_sessions(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    headers: HeaderMap,
    Query(query): Query<ListSessionsQuery>,
) -> Json<Vec<WireSession>> {
    let request_id = request_id_from_headers(&headers);
    let started = Instant::now();
    let workspace_from_query = query
        .workspace
        .as_deref()
        .and_then(tandem_core::normalize_workspace_path);
    let workspace_from_runtime = {
        let snapshot = state.workspace_index.snapshot().await;
        tandem_core::normalize_workspace_path(&snapshot.root)
    };
    let effective_scope = query.scope.unwrap_or_else(|| {
        if workspace_from_query.is_some() || workspace_from_runtime.is_some() {
            SessionScope::Workspace
        } else {
            SessionScope::Global
        }
    });
    let mut sessions = match effective_scope {
        SessionScope::Global => {
            state
                .storage
                .list_session_summaries_scoped(tandem_core::SessionListScope::Global)
                .await
        }
        SessionScope::Workspace => {
            let workspace = workspace_from_query.or(workspace_from_runtime);
            match workspace {
                Some(workspace_root) => {
                    state
                        .storage
                        .list_session_summaries_scoped(tandem_core::SessionListScope::Workspace {
                            workspace_root,
                        })
                        .await
                }
                None => Vec::new(),
            }
        }
    };
    sessions.retain(|session| session_visible_to_actor(&tenant_context, &session.tenant_context));
    let total_after_scope = sessions.len();
    sessions.sort_by(|a, b| b.time.updated.cmp(&a.time.updated));

    if let Some(archived) = query.archived {
        let mut filtered = Vec::new();
        for session in sessions {
            let status = state.storage.session_status(&session.id).await;
            let is_archived = status
                .as_ref()
                .and_then(|v| v.get("archived"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_archived == archived {
                filtered.push(session);
            }
        }
        sessions = filtered;
    }
    if let Some(q) = query.q.as_ref() {
        let q_lower = q.to_lowercase();
        sessions.retain(|session| {
            session.title.to_lowercase().contains(&q_lower)
                || session.directory.to_lowercase().contains(&q_lower)
        });
    }
    retain_sessions_for_source(&mut sessions, query.source.as_deref());

    let page_size = query.page_size.unwrap_or(20).max(1);
    let page = query.page.unwrap_or(1).max(1);
    let start = (page - 1) * page_size;
    let items = sessions
        .into_iter()
        .skip(start)
        .take(page_size)
        .map(session_with_effective_source_kind)
        .map(Into::into)
        .collect::<Vec<WireSession>>();
    let elapsed_ms = started.elapsed().as_millis();
    tracing::info!(
        "session.list request_id={} scope={:?} matched={} returned={} page={} page_size={} elapsed_ms={}",
        request_id,
        effective_scope,
        total_after_scope,
        items.len(),
        page,
        page_size,
        elapsed_ms
    );
    if elapsed_ms >= 1_000 {
        tracing::warn!(
            "slow request request_id={} route=GET /session elapsed_ms={} scope={:?} archived_filter={}",
            request_id,
            elapsed_ms,
            effective_scope,
            query.archived.is_some()
        );
    }
    Json(items)
}

pub(super) async fn attach_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(input): Json<AttachSessionInput>,
) -> Result<Json<WireSession>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let reason = input
        .reason_tag
        .unwrap_or_else(|| "manual_attach".to_string());
    let session = state
        .storage
        .attach_session_to_workspace(&id, &input.target_workspace, &reason)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    publish_tenant_event(
        &state,
        &session.tenant_context,
        "session.attached",
        json!({
            "sessionID": session.id,
            "workspaceRoot": session.workspace_root,
            "attachedFromWorkspace": session.attached_from_workspace,
            "attachedToWorkspace": session.attached_to_workspace,
            "attachReason": session.attach_reason
        }),
    );
    Ok(Json(session.into()))
}

pub(super) async fn grant_workspace_override(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(input): Json<WorkspaceOverrideInput>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let ttl = input.ttl_seconds.unwrap_or(900).clamp(30, 86_400);
    let expires_at = state
        .engine_loop
        .grant_workspace_override_for_session(&id, ttl)
        .await;
    publish_tenant_event(
        &state,
        &session.tenant_context,
        "session.workspace_override.granted",
        json!({
            "sessionID": id,
            "ttlSeconds": ttl,
            "expiresAtMs": expires_at
        }),
    );
    Ok(Json(json!({
        "ok": true,
        "ttlSeconds": ttl,
        "expiresAtMs": expires_at
    })))
}

pub(super) async fn get_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<WireSession>, StatusCode> {
    let request_id = request_id_from_headers(&headers);
    let started = Instant::now();
    let result = match state.storage.get_session(&id).await {
        Some(session) => ensure_same_session_actor(&tenant_context, &session.tenant_context)
            .map(|_| Json(session_with_effective_source_kind(session).into())),
        None => Err(StatusCode::NOT_FOUND),
    };
    let elapsed_ms = started.elapsed().as_millis();
    let status = if result.is_ok() { "ok" } else { "not_found" };
    tracing::info!(
        "session.get request_id={} session_id={} status={} elapsed_ms={}",
        request_id,
        id,
        status,
        elapsed_ms
    );
    if elapsed_ms >= 500 {
        tracing::warn!(
            "slow request request_id={} route=GET /session/{{id}} session_id={} elapsed_ms={}",
            request_id,
            id,
            elapsed_ms
        );
    }
    result
}

pub(super) async fn delete_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;

    if let Some(active_run) = state.run_registry.get(&id).await {
        let cancel_requested = state.cancellations.cancel_or_defer(&id).await;
        let active_run_id = active_run.run_id.clone();
        publish_tenant_event(
            &state,
            &session.tenant_context,
            "session.delete.deferred",
            json!({
                "sessionID": id,
                "runID": active_run_id,
                "cancelRequested": cancel_requested,
                "reason": "active_run",
            }),
        );
        return Ok(Json(json!({
            "deleted": false,
            "cancelRequested": cancel_requested,
            "activeRun": active_run,
            "reason": "active_run",
        })));
    }

    let deleted = state
        .storage
        .delete_session(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"deleted": deleted})))
}

pub(super) async fn session_messages(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let messages = session
        .messages
        .iter()
        .map(|msg| WireSessionMessage::from_message(msg, &id))
        .collect::<Vec<_>>();
    Ok(Json(json!(messages)))
}

pub(super) async fn prompt_async(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Query(query): Query<PromptAsyncQuery>,
    headers: HeaderMap,
    Json(req): Json<SendMessageRequest>,
) -> Result<Response, HttpError> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or_else(session_not_found_error)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)
        .map_err(|_| session_not_found_error())?;
    let session_id = id.clone();
    let correlation_id = headers
        .get("x-tandem-correlation-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let client_id = headers
        .get("x-tandem-client-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let run_id = Uuid::new_v4().to_string();
    let linked_context_run_id = super::context_runs::ensure_session_context_run(&state, &session)
        .await
        .map_err(|_| persistence_error("Failed to create linked context run"))?;

    let active_run = match state
        .run_registry
        .acquire(
            &session_id,
            run_id.clone(),
            client_id.clone(),
            req.agent.clone(),
            req.agent.clone(),
        )
        .await
    {
        Ok(run) => run,
        Err(active) => {
            let payload = conflict_payload(&session_id, &active);
            publish_tenant_event(
                &state,
                &session.tenant_context,
                "session.run.conflict",
                json!({
                    "sessionID": session_id,
                    "runID": active.run_id,
                    "retryAfterMs": 500,
                    "attachEventStream": attach_event_stream_path(&id, &active.run_id),
                }),
            );
            return Ok((StatusCode::CONFLICT, Json(payload)).into_response());
        }
    };

    tracing::info!(
        target: "tandem.obs",
        event = "server.prompt_async.start",
        component = "http.prompt_async",
        session_id = %session_id,
        correlation_id = %correlation_id.as_deref().unwrap_or(""),
        "prompt_async request accepted"
    );
    publish_tenant_event(
        &state,
        &session.tenant_context,
        "session.run.started",
        json!({
            "sessionID": session_id,
            "runID": active_run.run_id,
            "startedAtMs": active_run.started_at_ms,
            "clientID": active_run.client_id,
            "agentID": active_run.agent_id,
            "agentProfile": active_run.agent_profile,
            "environment": state.host_runtime_context(),
        }),
    );

    spawn_run_task(
        state.clone(),
        id.clone(),
        run_id.clone(),
        req,
        correlation_id,
        client_id,
        session.tenant_context.clone(),
    );

    if query.r#return.as_deref() == Some("run") {
        let mut response = (
            StatusCode::ACCEPTED,
            Json(json!({
                "runID": run_id,
                "contextRunID": linked_context_run_id,
                "linked_context_run_id": linked_context_run_id,
                "attachEventStream": attach_event_stream_path(&id, &run_id),
            })),
        )
            .into_response();
        if let Ok(value) = HeaderValue::from_str(&run_id) {
            response.headers_mut().insert("x-tandem-run-id", value);
        }
        return Ok(response);
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&run_id) {
        response.headers_mut().insert("x-tandem-run-id", value);
    }
    Ok(response)
}

pub(super) async fn prompt_sync(
    State(state): State<AppState>,
    Extension(request_tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<SendMessageRequest>,
) -> Result<Response, HttpError> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or_else(session_not_found_error)?;
    ensure_same_session_actor(&request_tenant_context, &session.tenant_context)
        .map_err(|_| session_not_found_error())?;
    if session.source_kind.as_deref() == Some("channel") {
        if let Some(key) =
            channel_rate_limit_key_from_session_metadata(session.source_metadata.as_ref())
        {
            let decision = state
                .channel_rate_limiter
                .check(
                    &key,
                    ChannelRateLimitKind::Prompt,
                    tandem_channels::config::ChannelSecurityProfile::PublicDemo,
                )
                .await;
            if !decision.allowed {
                let mut response = (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(ErrorEnvelope::new(
                        "Prompt rate limit exceeded",
                        ErrorCode::RateLimited,
                    )),
                )
                    .into_response();
                if let Ok(value) =
                    HeaderValue::from_str(&retry_after_duration(decision).as_secs().to_string())
                {
                    response.headers_mut().insert(header::RETRY_AFTER, value);
                }
                return Ok(response);
            }
        }
    }
    let accept_sse = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/event-stream"))
        .unwrap_or(false);
    let correlation_id = headers
        .get("x-tandem-correlation-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let client_id = headers
        .get("x-tandem-client-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let agent_id = headers
        .get("x-tandem-agent-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| req.agent.clone());
    let agent_profile = req.agent.clone();
    let tenant_context = session.tenant_context.clone();
    let run_id = Uuid::new_v4().to_string();
    let active_run = match state
        .run_registry
        .acquire(
            &id,
            run_id.clone(),
            client_id.clone(),
            agent_id.clone(),
            agent_profile.clone(),
        )
        .await
    {
        Ok(run) => run,
        Err(active) => {
            let payload = conflict_payload(&id, &active);
            publish_tenant_event(
                &state,
                &tenant_context,
                "session.run.conflict",
                json!({
                    "sessionID": id,
                    "runID": active.run_id,
                    "retryAfterMs": 500,
                    "attachEventStream": attach_event_stream_path(&id, &active.run_id),
                }),
            );
            return Ok((StatusCode::CONFLICT, Json(payload)).into_response());
        }
    };
    publish_tenant_event(
        &state,
        &tenant_context,
        "session.run.started",
        json!({
            "sessionID": id,
            "runID": active_run.run_id,
            "startedAtMs": active_run.started_at_ms,
            "clientID": active_run.client_id,
            "agentID": active_run.agent_id,
            "agentProfile": active_run.agent_profile,
            "environment": state.host_runtime_context(),
        }),
    );

    if accept_sse {
        spawn_run_task(
            state.clone(),
            id.clone(),
            run_id.clone(),
            req,
            correlation_id,
            client_id,
            tenant_context.clone(),
        );
        let stream = sse_run_stream(
            state.clone(),
            id.clone(),
            run_id.clone(),
            agent_id.clone(),
            agent_profile.clone(),
            tenant_context.clone(),
        );
        return Ok(Sse::new(stream)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
            .into_response());
    }

    let mut finished_events = state.event_bus.subscribe();
    spawn_run_task(
        state.clone(),
        id.clone(),
        run_id.clone(),
        req,
        correlation_id,
        client_id,
        tenant_context.clone(),
    );
    if !wait_for_run_finished_event(
        &state,
        &mut finished_events,
        &id,
        &run_id,
        Duration::from_secs(60 * 10 + 15),
    )
    .await
    {
        let _ = state.cancellations.cancel(&id).await;
        return Err(http_error(
            StatusCode::GATEWAY_TIMEOUT,
            "Prompt run timed out",
            ErrorCode::PromptTimeout,
        ));
    }
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or_else(session_not_found_error)?;
    let messages = session
        .messages
        .iter()
        .map(|msg| WireSessionMessage::from_message(msg, &id))
        .collect::<Vec<_>>();
    Ok(Json(json!(messages)).into_response())
}

pub(super) fn spawn_run_task(
    state: AppState,
    session_id: String,
    run_id: String,
    req: SendMessageRequest,
    correlation_id: Option<String>,
    client_id: Option<String>,
    tenant_context: TenantContext,
) {
    tokio::spawn(async move {
        let _ = execute_run(
            state,
            session_id,
            run_id,
            req,
            correlation_id,
            client_id,
            tenant_context,
        )
        .await;
    });
}
pub(super) async fn execute_run(
    state: AppState,
    session_id: String,
    run_id: String,
    mut req: SendMessageRequest,
    correlation_id: Option<String>,
    _client_id: Option<String>,
    tenant_context: TenantContext,
) -> anyhow::Result<()> {
    let kb_grounding_policy = derive_session_kb_grounding_policy(&state, &req).await;
    let strict_kb_model_override = req.model.clone();
    let mut direct_kb_outcome = None;
    let session = state.storage.get_session(&session_id).await;
    let verified_tenant_context = session.and_then(|session| session.verified_tenant_context);
    if let Some(policy) = kb_grounding_policy.as_ref() {
        let kb_tool_allowlist = tool_allowlist_for_kb_grounding(&policy);
        state
            .engine_loop
            .set_session_kb_grounding_policy(&session_id, policy.clone())
            .await;
        req.tool_mode = Some(ToolMode::Required);
        req.tool_allowlist = Some(kb_tool_allowlist.clone());
        publish_tenant_event(
            &state,
            &tenant_context,
            "kb.grounding.required",
            json!({
                "sessionID": session_id,
                "runID": run_id,
                "strict": policy.strict,
                "serverNames": policy.server_names,
                "toolPatterns": policy.tool_patterns,
                "toolAllowlist": kb_tool_allowlist,
            }),
        );
        if policy.strict && request_is_text_only(&req) {
            if let Some(tool_name) = policy_answer_question_tool(policy) {
                let question = send_message_request_text(&req);
                let args = json!({
                    "question": question,
                    "max_documents": 3,
                    "__phase_tool_authority": {
                        "phase": "kb_grounding",
                        "allowed_tools": kb_tool_allowlist,
                        "run_id": run_id,
                        "session_id": session_id,
                        "policy_id": "workflow_phase_tool_authority"
                    }
                });
                for server_name in &policy.server_names {
                    match super::mcp::call_mcp_tool_for_tenant_with_verified_context(
                        &state,
                        server_name,
                        &tool_name,
                        args.clone(),
                        &tenant_context,
                        verified_tenant_context.as_ref(),
                    )
                    .await
                    {
                        Ok(result) => {
                            let output = result
                                .metadata
                                .get("result")
                                .map(|value| {
                                    value
                                        .as_str()
                                        .map(ToOwned::to_owned)
                                        .unwrap_or_else(|| value.to_string())
                                })
                                .unwrap_or(result.output);
                            let namespaced_tool = format!(
                                "mcp.{}.{}",
                                mcp_namespace_segment_for_grounding(server_name),
                                tool_name
                            );
                            if let Some((answer, outcome)) = render_strict_kb_direct_answer(
                                &state,
                                &question,
                                &namespaced_tool,
                                &output,
                                policy,
                                strict_kb_model_override.as_ref(),
                            )
                            .await
                            {
                                persist_direct_kb_answer_messages(
                                    &state,
                                    &session_id,
                                    &question,
                                    &namespaced_tool,
                                    args.clone(),
                                    &output,
                                    &answer,
                                )
                                .await?;
                                direct_kb_outcome = Some(outcome);
                                tracing::info!(
                                    prefix = "STRICT_KB_DIRECT_ANSWER",
                                    session_id = %session_id,
                                    run_id = %run_id,
                                    server = %server_name,
                                    tool = %namespaced_tool,
                                    "STRICT_KB_DIRECT_ANSWER"
                                );
                                publish_tenant_event(
                                    &state,
                                    &tenant_context,
                                    "kb.grounding.strict.direct_answer",
                                    json!({
                                        "sessionID": session_id,
                                        "runID": run_id,
                                        "serverName": server_name,
                                        "tool": namespaced_tool,
                                    }),
                                );
                                break;
                            }
                        }
                        Err(error) => {
                            tracing::warn!(
                                server = %server_name,
                                tool = %tool_name,
                                error = %error,
                                "strict KB direct answer_question call failed"
                            );
                        }
                    }
                }
            }
        }
    } else {
        state
            .engine_loop
            .clear_session_kb_grounding_policy(&session_id)
            .await;
    }
    let (status, error_msg): (&str, Option<String>) = if direct_kb_outcome.is_some() {
        ("completed", None)
    } else {
        // Wrap the run so an expired provider token (AUTHENTICATION_ERROR) is
        // refreshed and retried once transparently before surfacing (TAN-595).
        // The timeout/ticker below poll this future, so the retry shares the
        // same run budget and keeps emitting heartbeats.
        let mut run_fut = Box::pin(super::session_run_retry::run_prompt_with_auth_retry(
            &state,
            &session_id,
            &run_id,
            req,
            correlation_id.clone(),
            &tenant_context,
        ));
        let mut timeout = Box::pin(tokio::time::sleep(Duration::from_secs(60 * 10)));
        let mut ticker = tokio::time::interval(Duration::from_secs(2));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    state.run_registry.touch(&session_id, &run_id).await;
                }
                _ = &mut timeout => {
                    let _ = state.cancellations.cancel(&session_id).await;
                    let timeout_text = "ENGINE_ERROR: ENGINE_TIMEOUT: prompt_async timed out";
                    let _ = persist_session_error_message(&state, &session_id, timeout_text).await;
                    publish_tenant_event(
                        &state,
                        &tenant_context,
                        "session.error",
                        json!({
                            "sessionID": session_id,
                            "error": {
                                "code": "ENGINE_TIMEOUT",
                                "message": "prompt_async timed out",
                            }
                        }),
                    );
                    publish_tenant_event(
                        &state,
                        &tenant_context,
                        "session.status",
                        json!({"sessionID": session_id, "status":"error"}),
                    );
                    publish_tenant_event(
                        &state,
                        &tenant_context,
                        "session.updated",
                        json!({"sessionID": session_id, "status":"error"}),
                    );
                    break ("timeout", Some("prompt_async timed out".to_string()));
                }
                result = &mut run_fut => {
                    match result {
                        Ok(()) => break ("completed", None),
                        Err(err) => {
                            let error_message = err.to_string();
                            let error_code = dispatch_error_code(&error_message);
                            let session_error_text =
                                format!("ENGINE_ERROR: {error_code}: {}", truncate_text(&error_message, 500));
                            let _ = persist_session_error_message(&state, &session_id, &session_error_text).await;
                            publish_tenant_event(
                                &state,
                                &tenant_context,
                                "session.error",
                                json!({
                                    "sessionID": session_id,
                                    "error": {
                                        "code": error_code,
                                        "message": truncate_text(&error_message, 500),
                                    }
                                }),
                            );
                            publish_tenant_event(
                                &state,
                                &tenant_context,
                                "session.status",
                                json!({"sessionID": session_id, "status":"error"}),
                            );
                            publish_tenant_event(
                                &state,
                                &tenant_context,
                                "session.updated",
                                json!({"sessionID": session_id, "status":"error"}),
                            );
                            let _ = state.cancellations.cancel(&session_id).await;
                            break ("error", Some(truncate_text(&error_message, 500)));
                        }
                    }
                }
            }
        }
    };

    if let Some(outcome) = direct_kb_outcome {
        publish_tenant_event(
            &state,
            &tenant_context,
            "kb.grounding.strict.applied",
            json!({
                "sessionID": session_id,
                "runID": run_id,
                "support": outcome.support,
                "sources": outcome.sources,
                "evidenceCount": outcome.evidence_count,
            }),
        );
    } else if status == "completed" || strict_kb_should_repair_error(error_msg.as_deref()) {
        if let Some(policy) = kb_grounding_policy.as_ref().filter(|policy| policy.strict) {
            match apply_strict_kb_grounding_after_run(
                &state,
                &session_id,
                policy,
                strict_kb_model_override,
            )
            .await
            {
                Ok(Some(outcome)) => {
                    publish_tenant_event(
                        &state,
                        &tenant_context,
                        "kb.grounding.strict.applied",
                        json!({
                            "sessionID": session_id,
                            "runID": run_id,
                            "support": outcome.support,
                            "sources": outcome.sources,
                            "evidenceCount": outcome.evidence_count,
                        }),
                    );
                }
                Ok(None) => {}
                Err(error) => {
                    publish_tenant_event(
                        &state,
                        &tenant_context,
                        "kb.grounding.strict.error",
                        json!({
                            "sessionID": session_id,
                            "runID": run_id,
                            "error": truncate_text(&error.to_string(), 500),
                        }),
                    );
                }
            }
        }
    }

    let _ = state
        .run_registry
        .finish_if_match(&session_id, &run_id)
        .await;
    publish_tenant_event(
        &state,
        &tenant_context,
        "session.run.finished",
        json!({
            "sessionID": session_id,
            "runID": run_id,
            "finishedAtMs": crate::now_ms(),
            "status": status,
            "error": error_msg,
        }),
    );

    if status == "completed" {
        let session_id_clone = session_id.clone();
        let state_clone = state.clone();
        tokio::spawn(async move {
            archive_session_exchange_to_global_memory(state_clone, session_id_clone).await;
        });
    }

    Ok(())
}

async fn persist_session_error_message(
    state: &AppState,
    session_id: &str,
    text: &str,
) -> anyhow::Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let msg = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::Text {
            text: text.trim().to_string(),
        }],
    );
    state.storage.append_message(session_id, msg).await
}

async fn persist_direct_kb_answer_messages(
    state: &AppState,
    session_id: &str,
    question: &str,
    tool_name: &str,
    tool_args: Value,
    tool_output: &str,
    answer: &str,
) -> anyhow::Result<()> {
    if question.trim().is_empty() || answer.trim().is_empty() {
        return Ok(());
    }
    let user_message = Message::new(
        MessageRole::User,
        vec![
            MessagePart::Text {
                text: question.trim().to_string(),
            },
            MessagePart::ToolInvocation {
                tool: tool_name.to_string(),
                args: tool_args,
                result: Some(Value::String(tool_output.to_string())),
                error: None,
            },
        ],
    );
    state
        .storage
        .append_message(session_id, user_message)
        .await?;
    let assistant_message = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::Text {
            text: answer.trim().to_string(),
        }],
    );
    state
        .storage
        .append_message(session_id, assistant_message)
        .await
}

pub(super) fn sse_run_stream(
    state: AppState,
    session_id: String,
    run_id: String,
    agent_id: Option<String>,
    agent_profile: Option<String>,
    tenant_context: TenantContext,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let rx = state.event_bus.subscribe();
    let started_event = EngineEvent::new(
        "session.run.started",
        with_tenant_context(
            json!({
                "sessionID": session_id,
                "runID": run_id,
                "startedAtMs": crate::now_ms(),
                "agentID": agent_id,
                "agentProfile": agent_profile,
                "channel": "system",
                "environment": state.host_runtime_context(),
            }),
            &tenant_context,
        ),
    );
    let started = tokio_stream::once(Ok(
        Event::default().data(serde_json::to_string(&started_event).unwrap_or_default())
    ));
    let filter_session_id = session_id.clone();
    let filter_run_id = run_id.clone();
    let end_run_id = run_id.clone();
    let map_session_id = session_id.clone();
    let map_run_id = run_id.clone();

    let run_events = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) if event_matches_run(&event, &filter_session_id, &filter_run_id) => Some(event),
        _ => None,
    });
    let live = run_events.take_while(move |event| {
        let is_finished = event.event_type == "session.run.finished"
            && event
                .properties
                .get("runID")
                .and_then(|v| v.as_str())
                .map(|v| v == end_run_id.as_str())
                .unwrap_or(false);
        !is_finished
    });
    let mapped = live.map(move |event| {
        let normalized = normalize_run_event(event, &map_session_id, &map_run_id, &tenant_context);
        let payload = serde_json::to_string(&normalized).unwrap_or_default();
        Ok(Event::default().data(payload))
    });
    started.chain(mapped)
}

pub(super) fn conflict_payload(session_id: &str, active: &ActiveRun) -> Value {
    json!({
        "error": "Session already has an active run",
        "code": ErrorCode::SessionRunConflict,
        "retryable": true,
        "sessionID": session_id,
        "activeRun": {
            "runID": active.run_id,
            "startedAtMs": active.started_at_ms,
            "lastActivityAtMs": active.last_activity_at_ms,
            "clientID": active.client_id,
            "agentID": active.agent_id,
            "agentProfile": active.agent_profile,
        },
        "retryAfterMs": 500,
        "attachEventStream": attach_event_stream_path(session_id, &active.run_id),
    })
}

pub(super) fn attach_event_stream_path(session_id: &str, run_id: &str) -> String {
    format!("/event?sessionID={session_id}&runID={run_id}")
}

pub(super) fn event_matches_run(event: &EngineEvent, session_id: &str, run_id: &str) -> bool {
    let event_session = event
        .properties
        .get("sessionID")
        .or_else(|| event.properties.get("sessionId"))
        .or_else(|| event.properties.get("id"))
        .and_then(|v| v.as_str());
    if event_session != Some(session_id) {
        return false;
    }
    let event_run = event
        .properties
        .get("runID")
        .or_else(|| event.properties.get("run_id"))
        .and_then(|v| v.as_str());
    match event_run {
        Some(value) => value == run_id,
        None => true,
    }
}

pub(super) fn normalize_run_event(
    mut event: EngineEvent,
    session_id: &str,
    run_id: &str,
    tenant_context: &TenantContext,
) -> EngineEvent {
    if !event.properties.is_object() {
        event.properties = json!({});
    }
    if let Some(props) = event.properties.as_object_mut() {
        if !props.contains_key("sessionID") {
            props.insert("sessionID".to_string(), json!(session_id));
        }
        if !props.contains_key("runID") {
            props.insert("runID".to_string(), json!(run_id));
        }
        if !props.contains_key("tenantContext") {
            props.insert(
                "tenantContext".to_string(),
                tenant_context_event_value(tenant_context),
            );
        }
        if !props.contains_key("agentID") {
            if let Some(agent) = props.get("agent").and_then(|v| v.as_str()) {
                props.insert("agentID".to_string(), json!(agent));
            }
        }
        if !props.contains_key("channel") {
            let channel = infer_event_channel(&event.event_type, props);
            props.insert("channel".to_string(), json!(channel));
        }
    }
    event
}

pub(super) fn infer_event_channel(
    event_type: &str,
    props: &serde_json::Map<String, Value>,
) -> &'static str {
    if event_type.starts_with("session.") {
        return "system";
    }
    if event_type.starts_with("todo.") || event_type.starts_with("question.") {
        return "system";
    }
    if event_type == "message.part.updated" {
        if let Some(part_type) = props
            .get("part")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str())
        {
            if part_type == "tool-invocation" || part_type == "tool-result" {
                return "tool";
            }
        }
        return "assistant";
    }
    "log"
}

pub(super) fn dispatch_error_code(message: &str) -> &'static str {
    let lower = message.to_ascii_lowercase();
    if is_os_mismatch_error(message) {
        return "OS_MISMATCH";
    }
    if lower.contains("rate limit") || lower.contains("too many requests") || lower.contains("429")
    {
        return "RATE_LIMIT_EXCEEDED";
    }
    if lower.contains("context length")
        || lower.contains("max tokens")
        || lower.contains("token limit")
    {
        return "CONTEXT_LENGTH_EXCEEDED";
    }
    if lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("user not found")
        || lower.contains("invalid api key")
        || lower.contains("401")
        || lower.contains("403")
    {
        return "AUTHENTICATION_ERROR";
    }
    if lower.contains("provider_server_error")
        || lower.contains("internal server error")
        || lower.contains("provider stream chunk error")
        || lower.contains("json error injected into sse stream")
    {
        return "PROVIDER_SERVER_ERROR";
    }
    if message.contains("invalid_function_parameters")
        || message.contains("array schema missing items")
    {
        "TOOL_SCHEMA_INVALID"
    } else {
        "ENGINE_DISPATCH_FAILED"
    }
}

fn strict_kb_should_repair_error(error: Option<&str>) -> bool {
    let Some(error) = error else {
        return false;
    };
    let lower = error.to_ascii_lowercase();
    lower.contains("provider stream chunk error")
        || lower.contains("error decoding response body")
        || lower.contains("incomplete streamed response")
        || lower.contains("provider_server_error")
        || lower.contains("provider server error")
        || lower.contains("unexpected eof")
}

pub(super) fn is_os_mismatch_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("os error 3")
        || lower.contains("system cannot find the path specified")
        || lower.contains("cannot find path")
        || lower.contains("is not recognized as an internal or external command")
        || lower.contains("no such file or directory")
        || lower.contains("command not found")
}

pub(super) fn truncate_text(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_len {
            break;
        }
        end = next;
    }
    let mut out = input[..end].to_string();
    out.push_str("...<truncated>");
    out
}

pub(super) async fn append_message_only(
    state: &AppState,
    session_id: &str,
    req: SendMessageRequest,
) -> Result<WireSessionMessage, String> {
    if state.storage.get_session(session_id).await.is_none() {
        return Err("session not found".to_string());
    }
    let text = req
        .parts
        .iter()
        .map(|p| match p {
            MessagePartInput::Text { text } => text.clone(),
            MessagePartInput::File {
                mime,
                filename,
                url,
            } => format!(
                "[file mime={} name={} url={}]",
                mime,
                filename.clone().unwrap_or_else(|| "unknown".to_string()),
                url
            ),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let msg = Message::new(
        MessageRole::User,
        vec![MessagePart::Text { text: text.clone() }],
    );
    let wire = WireSessionMessage::from_message(&msg, session_id);
    state
        .storage
        .append_message(session_id, msg)
        .await
        .map_err(|e| format!("{e:#}"))?;

    if let Some(mut session) = state.storage.get_session(session_id).await {
        if tandem_core::title_needs_repair(&session.title) {
            let first_user_text = session.messages.iter().find_map(|message| {
                if !matches!(message.role, MessageRole::User) {
                    return None;
                }
                message.parts.iter().find_map(|part| match part {
                    MessagePart::Text { text } if !text.trim().is_empty() => Some(text.clone()),
                    _ => None,
                })
            });
            let title_source = first_user_text.unwrap_or_else(|| text.clone());
            if let Some(new_title) =
                tandem_core::derive_session_title_from_prompt(&title_source, 60)
            {
                session.title = new_title;
                session.time.updated = chrono::Utc::now();
                let _ = state.storage.save_session(session).await;
            }
        }
    }

    Ok(wire)
}

pub(super) async fn session_todos(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let todos = state
        .storage
        .get_todos(&id)
        .await
        .into_iter()
        .filter_map(|v| serde_json::from_value::<TodoItem>(v).ok())
        .collect::<Vec<_>>();
    Ok(Json(json!(todos)))
}

pub(super) async fn session_status_handler(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Json<Value> {
    let sessions = state
        .storage
        .list_session_summaries_scoped(tandem_core::SessionListScope::Global)
        .await;
    let mut map = serde_json::Map::new();
    for s in sessions {
        if !session_visible_to_actor(&tenant_context, &s.tenant_context) {
            continue;
        }
        let mut status = json!({"type":"idle"});
        if let Some(meta) = state.storage.session_status(&s.id).await {
            status["meta"] = meta;
        }
        map.insert(s.id, status);
    }
    Json(Value::Object(map))
}

pub(super) async fn update_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(input): Json<UpdateSessionInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    if let Some(title) = input.title {
        session.title = title;
    }
    apply_session_permission_rules(&state, input.permission).await;
    session.time.updated = chrono::Utc::now();
    state
        .storage
        .save_session(session.clone())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(archived) = input.archived {
        state
            .storage
            .set_archived(&id, archived)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(Json(json!(session)))
}

pub(super) async fn post_session_message_append(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Response, (StatusCode, String)> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or((StatusCode::NOT_FOUND, "session not found".to_string()))?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)
        .map_err(|status| (status, "session not found".to_string()))?;
    let wire = append_message_only(&state, &id, req)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok(Json(wire).into_response())
}

pub(super) async fn get_active_run(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let linked_context_run_id =
        super::context_runs::ensure_session_context_run(&state, &session).await?;
    let active = state.run_registry.get(&id).await;
    match active {
        Some(run) => Ok(Json(json!({
            "active": run,
            "contextRunID": linked_context_run_id,
            "linked_context_run_id": linked_context_run_id,
        }))),
        None => Ok(Json(json!({ "active": Value::Null }))),
    }
}

pub(super) async fn abort_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    // GOV-B2d: aborting a session cancels in-flight work, so attribute and audit it.
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let cancelled = state.cancellations.cancel(&id).await;
    let cancelled_run = state.run_registry.finish_active(&id).await;
    let closed_browser_sessions = state.close_browser_sessions_for_owner(&id).await;
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "session.aborted",
        &tenant_context,
        actor.actor_id.clone().or_else(|| actor.source.clone()),
        json!({
            "sessionID": id,
            "cancelled": cancelled || cancelled_run.is_some(),
            "runID": cancelled_run.as_ref().map(|run| run.run_id.clone()),
            "closedBrowserSessions": closed_browser_sessions,
            "actor": actor.clone(),
        }),
    )
    .await;
    if let Some(run) = cancelled_run.as_ref() {
        if let Some(session) = state.storage.get_session(&id).await {
            publish_tenant_event(
                &state,
                &session.tenant_context,
                "session.run.finished",
                json!({
                    "sessionID": id,
                    "runID": run.run_id,
                    "finishedAtMs": crate::now_ms(),
                    "status": "cancelled",
                }),
            );
        }
    }
    Ok(Json(json!({
        "ok": true,
        "cancelled": cancelled || cancelled_run.is_some(),
        "closedBrowserSessions": closed_browser_sessions,
    })))
}

pub(super) async fn cancel_run_by_id(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path((id, run_id)): Path<(String, String)>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let active = state.run_registry.get(&id).await;
    if let Some(active_run) = active {
        if active_run.run_id == run_id {
            // GOV-B2d: attribute and audit the run cancellation.
            let actor = super::governance::resolve_governance_actor(
                &headers,
                &tenant_context,
                &request_principal,
            );
            let _cancelled = state.cancellations.cancel(&id).await;
            let _ = state.run_registry.finish_if_match(&id, &run_id).await;
            let closed_browser_sessions = state.close_browser_sessions_for_owner(&id).await;
            let _ = crate::audit::append_protected_audit_event(
                &state,
                "session.run.cancelled",
                &tenant_context,
                actor.actor_id.clone().or_else(|| actor.source.clone()),
                json!({
                    "sessionID": id,
                    "runID": run_id,
                    "closedBrowserSessions": closed_browser_sessions,
                    "actor": actor.clone(),
                }),
            )
            .await;
            if let Some(session) = state.storage.get_session(&id).await {
                publish_tenant_event(
                    &state,
                    &session.tenant_context,
                    "session.run.finished",
                    json!({
                        "sessionID": id,
                        "runID": run_id,
                        "finishedAtMs": crate::now_ms(),
                        "status": "cancelled",
                    }),
                );
            }
            return Ok(Json(json!({
                "ok": true,
                "cancelled": true,
                "closedBrowserSessions": closed_browser_sessions,
            })));
        }
    }
    Ok(Json(json!({"ok": true, "cancelled": false})))
}

include!("sessions_more.rs");

#[cfg(test)]
include!("sessions_tests.rs");
