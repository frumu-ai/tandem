// ============================================================================
// Message Handling
// ============================================================================

/// File attachment from frontend
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FileAttachmentInput {
    pub mime: String,
    pub filename: Option<String>,
    pub url: String,
}

fn default_memory_retrieval_meta() -> MemoryRetrievalMeta {
    MemoryRetrievalMeta {
        used: false,
        chunks_total: 0,
        session_chunks: 0,
        history_chunks: 0,
        project_fact_chunks: 0,
        score_min: None,
        score_max: None,
    }
}

fn should_skip_memory_retrieval(prompt: &str) -> bool {
    let trimmed = prompt.trim();
    trimmed.is_empty() || trimmed.starts_with('/')
}

fn is_embeddings_disabled_error(message: &str) -> bool {
    message.to_ascii_lowercase().contains("embeddings disabled")
}

fn short_query_hash(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return "none".to_string();
    }
    let mut hasher = Sha256::new();
    hasher.update(trimmed.as_bytes());
    let full = format!("{:x}", hasher.finalize());
    full.chars().take(12).collect()
}

fn build_message_content_with_memory_context(original: &str, memory_context: &str) -> String {
    if memory_context.trim().is_empty() {
        return original.to_string();
    }
    if original.is_empty() {
        return memory_context.to_string();
    }
    format!("{}\n\n{}", memory_context, original)
}

fn memory_retrieval_event(
    session_id: &str,
    status: &str,
    meta: &MemoryRetrievalMeta,
    latency_ms: u128,
    query_hash: String,
    embedding_status: Option<String>,
    embedding_reason: Option<String>,
) -> StreamEvent {
    StreamEvent::MemoryRetrieval {
        session_id: session_id.to_string(),
        status: Some(status.to_string()),
        used: meta.used,
        chunks_total: meta.chunks_total,
        session_chunks: meta.session_chunks,
        history_chunks: meta.history_chunks,
        project_fact_chunks: meta.project_fact_chunks,
        latency_ms,
        query_hash,
        score_min: meta.score_min,
        score_max: meta.score_max,
        embedding_status,
        embedding_reason,
    }
}

fn memory_storage_event(
    session_id: &str,
    message_id: Option<String>,
    role: &str,
    session_chunks_stored: usize,
    project_chunks_stored: usize,
    status: Option<String>,
    error: Option<String>,
) -> StreamEvent {
    StreamEvent::MemoryStorage {
        session_id: session_id.to_string(),
        message_id,
        role: role.to_string(),
        session_chunks_stored,
        project_chunks_stored,
        status,
        error,
    }
}

fn attachment_inputs_to_queued(
    attachments: Option<Vec<FileAttachmentInput>>,
) -> Vec<crate::state::QueuedAttachment> {
    attachments
        .unwrap_or_default()
        .into_iter()
        .map(|a| crate::state::QueuedAttachment {
            mime: a.mime,
            filename: a.filename,
            url: a.url,
        })
        .collect()
}

fn queued_to_attachment_inputs(
    attachments: Vec<crate::state::QueuedAttachment>,
) -> Option<Vec<FileAttachmentInput>> {
    if attachments.is_empty() {
        return None;
    }
    Some(
        attachments
            .into_iter()
            .map(|a| FileAttachmentInput {
                mime: a.mime,
                filename: a.filename,
                url: a.url,
            })
            .collect(),
    )
}

async fn emit_stream_event_pair(
    app: &AppHandle,
    tenant_context: &tandem_types::TenantContext,
    event: &StreamEvent,
    source: StreamEventSource,
    correlation_id: String,
) {
    if let Err(err) = crate::tool_history::record_stream_event(app, tenant_context, event) {
        tracing::warn!("Failed to persist stream event to tool history: {}", err);
    }
    let _ = app.emit("sidecar_event", event);
    let envelope = StreamEventEnvelopeV2 {
        event_id: Uuid::new_v4().to_string(),
        correlation_id,
        ts_ms: logs::now_ms(),
        tenant_context: tenant_context.clone(),
        session_id: match event {
            StreamEvent::Content { session_id, .. }
            | StreamEvent::ToolStart { session_id, .. }
            | StreamEvent::ToolEnd { session_id, .. }
            | StreamEvent::SessionStatus { session_id, .. }
            | StreamEvent::RunStarted { session_id, .. }
            | StreamEvent::RunFinished { session_id, .. }
            | StreamEvent::RunConflict { session_id, .. }
            | StreamEvent::SessionIdle { session_id }
            | StreamEvent::SessionError { session_id, .. }
            | StreamEvent::PermissionAsked { session_id, .. }
            | StreamEvent::QuestionAsked { session_id, .. }
            | StreamEvent::TodoUpdated { session_id, .. }
            | StreamEvent::FileEdited { session_id, .. }
            | StreamEvent::MemoryRetrieval { session_id, .. }
            | StreamEvent::MemoryStorage { session_id, .. } => Some(session_id.clone()),
            StreamEvent::Raw { .. } => None,
        },
        source,
        payload: event.clone(),
    };
    let _ = app.emit("sidecar_event_v2", envelope);
}

async fn prepare_prompt_with_memory_context(
    state: &AppState,
    session_id: &str,
    prompt_content: &str,
    retrieval_query: &str,
) -> (String, StreamEvent) {
    let query_hash = short_query_hash(retrieval_query);
    let embedding_health = if let Some(manager) = &state.memory_manager {
        let health = manager.embedding_health().await;
        (Some(health.status), health.reason)
    } else {
        (
            Some("unavailable".to_string()),
            Some("memory manager not initialized".to_string()),
        )
    };

    if should_skip_memory_retrieval(retrieval_query) {
        let meta = default_memory_retrieval_meta();
        tracing::info!(
            target: "tandem.memory",
            "ðŸ§  memory_retrieval status=skipped used={} chunks_total={} session_chunks={} history_chunks={} project_fact_chunks={} latency_ms={} query_hash={} score_min={:?} score_max={:?}",
            meta.used,
            meta.chunks_total,
            meta.session_chunks,
            meta.history_chunks,
            meta.project_fact_chunks,
            0u128,
            query_hash,
            meta.score_min,
            meta.score_max
        );
        return (
            prompt_content.to_string(),
            memory_retrieval_event(
                session_id,
                "not_attempted",
                &meta,
                0,
                query_hash,
                embedding_health.0,
                embedding_health.1,
            ),
        );
    }

    let Some(manager) = &state.memory_manager else {
        let meta = default_memory_retrieval_meta();
        tracing::info!(
            target: "tandem.memory",
            "ðŸ§  memory_retrieval status=unavailable used={} chunks_total={} session_chunks={} history_chunks={} project_fact_chunks={} latency_ms={} query_hash={} score_min={:?} score_max={:?}",
            meta.used,
            meta.chunks_total,
            meta.session_chunks,
            meta.history_chunks,
            meta.project_fact_chunks,
            0u128,
            query_hash,
            meta.score_min,
            meta.score_max
        );
        return (
            prompt_content.to_string(),
            memory_retrieval_event(
                session_id,
                "error_fallback",
                &meta,
                0,
                query_hash,
                embedding_health.0,
                embedding_health.1,
            ),
        );
    };

    let resolved_project_id = resolve_memory_project_id_for_session(state, session_id).await;
    let started = Instant::now();
    let (final_content, meta, latency_ms, retrieval_status) = match manager
        .retrieve_context_with_meta(
            retrieval_query,
            resolved_project_id.as_deref(),
            Some(session_id),
            None,
        )
        .await
    {
        Ok((context, meta)) => {
            let context_text = context.format_for_injection();
            let merged = build_message_content_with_memory_context(prompt_content, &context_text);
            (
                merged,
                meta.clone(),
                started.elapsed().as_millis(),
                if meta.used {
                    "retrieved_used"
                } else {
                    "attempted_no_hits"
                },
            )
        }
        Err(e) => {
            let status = if e
                .to_string()
                .to_ascii_lowercase()
                .contains("embeddings disabled")
            {
                "degraded_disabled"
            } else {
                "error_fallback"
            };
            tracing::warn!(
                target: "tandem.memory",
                "ðŸ§  memory_retrieval status=error session_id={} error={}",
                session_id,
                e
            );
            (
                prompt_content.to_string(),
                default_memory_retrieval_meta(),
                started.elapsed().as_millis(),
                status,
            )
        }
    };

    tracing::info!(
        target: "tandem.memory",
        "ðŸ§  memory_retrieval status=ok used={} chunks_total={} session_chunks={} history_chunks={} project_fact_chunks={} latency_ms={} query_hash={} score_min={:?} score_max={:?}",
        meta.used,
        meta.chunks_total,
        meta.session_chunks,
        meta.history_chunks,
        meta.project_fact_chunks,
        latency_ms,
        query_hash,
        meta.score_min,
        meta.score_max
    );

    (
        final_content,
        memory_retrieval_event(
            session_id,
            retrieval_status,
            &meta,
            latency_ms,
            query_hash,
            embedding_health.0,
            embedding_health.1,
        ),
    )
}

async fn resolve_memory_project_id_for_session(
    state: &AppState,
    session_id: &str,
) -> Option<String> {
    if let Ok(session) = state.sidecar.get_session(session_id).await {
        if let Some(pid) = session.project_id {
            let trimmed = pid.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(workspace_root) = session.workspace_root {
            let normalized = normalize_workspace_path(&workspace_root)?;
            let projects = state.user_projects.read().unwrap();
            if let Some(project) = projects.iter().find(|p| {
                normalize_workspace_path(&p.path)
                    .map(|candidate| candidate == normalized)
                    .unwrap_or(false)
            }) {
                return Some(project.id.clone());
            }
        }
    }
    state.active_project_id.read().unwrap().clone()
}

async fn store_user_message_in_memory(
    app: &AppHandle,
    state: &AppState,
    session_id: &str,
    content: &str,
) {
    if should_skip_memory_retrieval(content) {
        return;
    }
    let Some(manager) = &state.memory_manager else {
        return;
    };
    let embedding_health = manager.embedding_health().await;
    let tenant_context = crate::state::resolve_session_tenant_context(state, session_id).await;
    if embedding_health.status != "ok" {
        tracing::info!(
            target: "tandem.memory",
            "Skipping user memory storage: session_id={} status={} reason={}",
            session_id,
            embedding_health.status,
            embedding_health.reason.as_deref().unwrap_or("unknown")
        );
        emit_stream_event_pair(
            app,
            &tenant_context,
            &memory_storage_event(
                session_id,
                None,
                "user",
                0,
                0,
                Some("degraded_disabled".to_string()),
                embedding_health.reason,
            ),
            StreamEventSource::Memory,
            format!("{}:memory-store:user:{}", session_id, Uuid::new_v4()),
        )
        .await;
        return;
    }
    let active_project_id = resolve_memory_project_id_for_session(state, session_id).await;
    let base_metadata = serde_json::json!({
        "role": "user",
        "source_kind": "chat_turn"
    });

    let session_req = StoreMessageRequest {
        content: content.to_string(),
        tier: MemoryTier::Session,
        session_id: Some(session_id.to_string()),
        project_id: active_project_id.clone(),
        source: "user_message".to_string(),
        source_path: None,
        source_mtime: None,
        source_size: None,
        source_hash: None,
        tenant_scope: MemoryTenantScope::local(),
        subject: None,
        metadata: Some(base_metadata.clone()),
    };
    let mut session_chunks_stored = 0usize;
    let mut project_chunks_stored = 0usize;
    let mut storage_error: Option<String> = None;
    let mut embeddings_disabled = false;

    match manager.store_message(session_req).await {
        Ok(ids) => {
            session_chunks_stored = ids.len();
        }
        Err(err) => {
            if is_embeddings_disabled_error(&err.to_string()) {
                embeddings_disabled = true;
                tracing::info!(
                    target: "tandem.memory",
                    "User session memory storage degraded (embeddings disabled): session_id={} error={}",
                    session_id,
                    err
                );
            } else {
                tracing::warn!(
                    target: "tandem.memory",
                    "Failed to store user session memory chunk: session_id={} error={}",
                    session_id,
                    err
                );
            }
            storage_error.get_or_insert_with(|| err.to_string());
        }
    }

    if let Some(project_id) = active_project_id {
        let project_req = StoreMessageRequest {
            content: content.to_string(),
            tier: MemoryTier::Project,
            session_id: Some(session_id.to_string()),
            project_id: Some(project_id.clone()),
            source: "user_message".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            tenant_scope: MemoryTenantScope::local(),
            subject: None,
            metadata: Some(base_metadata),
        };
        match manager.store_message(project_req).await {
            Ok(ids) => {
                project_chunks_stored = ids.len();
            }
            Err(err) => {
                if is_embeddings_disabled_error(&err.to_string()) {
                    embeddings_disabled = true;
                    tracing::info!(
                        target: "tandem.memory",
                        "User project memory storage degraded (embeddings disabled): session_id={} project_id={} error={}",
                        session_id,
                        project_id,
                        err
                    );
                } else {
                    tracing::warn!(
                        target: "tandem.memory",
                        "Failed to store user project memory chunk: session_id={} project_id={} error={}",
                        session_id,
                        project_id,
                        err
                    );
                }
                storage_error.get_or_insert_with(|| err.to_string());
            }
        }
    }

    emit_stream_event_pair(
        app,
        &tenant_context,
        &memory_storage_event(
            session_id,
            None,
            "user",
            session_chunks_stored,
            project_chunks_stored,
            Some(if embeddings_disabled {
                "degraded_disabled".to_string()
            } else if storage_error.is_some() {
                "error".to_string()
            } else {
                "ok".to_string()
            }),
            storage_error,
        ),
        StreamEventSource::Memory,
        format!("{}:memory-store:user:{}", session_id, Uuid::new_v4()),
    )
    .await;
}

/// Send a message to a session (async, starts generation)
/// The actual response comes via the event stream
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    attachments: Option<Vec<FileAttachmentInput>>,
) -> Result<()> {
    send_message_and_start_run_internal(
        &app,
        &state,
        session_id,
        content,
        attachments,
        None,
        None,
        None,
        None,
        false,
    )
    .await
}

/// Send a message and subscribe to events for the response
/// This emits events to the frontend as chunks arrive
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn send_message_and_start_run(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    attachments: Option<Vec<FileAttachmentInput>>,
    agent: Option<String>,
    mode_id: Option<String>,
    model: Option<String>,
    provider: Option<String>,
) -> Result<()> {
    send_message_and_start_run_internal(
        &app,
        &state,
        session_id,
        content,
        attachments,
        agent,
        mode_id,
        model,
        provider,
        true,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn send_message_and_start_run_internal(
    app: &AppHandle,
    state: &AppState,
    session_id: String,
    content: String,
    attachments: Option<Vec<FileAttachmentInput>>,
    agent: Option<String>,
    mode_id: Option<String>,
    model: Option<String>,
    provider: Option<String>,
    streaming_label: bool,
) -> Result<()> {
    let correlation_id = Uuid::new_v4().to_string();
    emit_event(
        tracing::Level::INFO,
        ProcessKind::Desktop,
        ObservabilityEvent {
            event: "chat.dispatch.start",
            component: "tauri.commands",
            org_id: None,
            workspace_id: None,
            correlation_id: Some(&correlation_id),
            session_id: Some(&session_id),
            run_id: None,
            message_id: None,
            provider_id: None,
            model_id: None,
            status: Some("start"),
            error_code: None,
            detail: Some("send_message_and_start_run_internal"),
        },
    );
    let mode_resolution = resolve_effective_mode(app, state, mode_id.as_deref(), agent.as_deref())?;
    if let Some(reason) = mode_resolution.fallback_reason.as_ref() {
        tracing::warn!("[send_message_and_start_run] {}", reason);
    }
    tracing::info!(
        "[send_message_and_start_run] session={} mode_id={} base_mode={:?} requested_agent={:?} resolved_sidecar_agent={:?}",
        session_id,
        mode_resolution.mode.id,
        mode_resolution.mode.base_mode,
        agent,
        mode_resolution.mode.sidecar_agent()
    );
    set_session_mode(state, &session_id, mode_resolution.mode.clone());

    store_user_message_in_memory(app, state, &session_id, &content).await;
    let retrieval_query = content.clone();
    let base_prompt = if let Some(extra) = mode_resolution.mode.system_prompt_append.as_deref() {
        format!(
            "[Mode instructions]\n{}\n\n[User request]\n{}",
            extra, content
        )
    } else {
        content
    };

    let (prepared_content, retrieval_event) =
        prepare_prompt_with_memory_context(state, &session_id, &base_prompt, &retrieval_query)
            .await;
    let tenant_context = crate::state::resolve_session_tenant_context(state, &session_id).await;
    emit_stream_event_pair(
        app,
        &tenant_context,
        &retrieval_event,
        StreamEventSource::Memory,
        format!("{}:memory:{}", session_id, Uuid::new_v4()),
    )
    .await;

    let mut request = if let Some(files) = attachments {
        let file_parts: Vec<FilePartInput> = files
            .into_iter()
            .map(|f| FilePartInput {
                part_type: "file".to_string(),
                mime: f.mime,
                filename: f.filename,
                url: f.url,
            })
            .collect();
        SendMessageRequest::with_attachments(prepared_content, file_parts)
    } else {
        SendMessageRequest::text(prepared_content)
    };

    let config_snapshot = { state.providers_config.read().unwrap().clone() };
    let model_spec = resolve_required_model_spec(
        &config_snapshot,
        model.clone(),
        provider.clone(),
        if streaming_label {
            "Streaming dispatch"
        } else {
            "Message dispatch"
        },
    )?;
    tracing::debug!(
        "Resolved model spec ({}): provider={} model={} (openrouter enabled={} default={} has_key={}, ollama enabled={} default={})",
        if streaming_label { "streaming" } else { "standard" },
        model_spec.provider_id,
        model_spec.model_id,
        config_snapshot.openrouter.enabled,
        config_snapshot.openrouter.default,
        config_snapshot.openrouter.has_key,
        config_snapshot.ollama.enabled,
        config_snapshot.ollama.default
    );

    {
        validate_model_provider_auth_if_required(
            app,
            &config_snapshot,
            Some(model_spec.model_id.as_str()),
            Some(model_spec.provider_id.as_str()),
        )
        .await?;
    }

    tracing::info!(
        "chat.dispatch.model session_id={} provider={} model={}",
        session_id,
        model_spec.provider_id,
        model_spec.model_id
    );

    request.model = Some(model_spec);

    if let Some(agent_name) = mode_resolution.mode.sidecar_agent() {
        request.agent = Some(agent_name);
    }

    match state
        .sidecar
        .append_message_and_start_run_with_context(&session_id, request, Some(&correlation_id))
        .await
    {
        Ok(()) => {
            emit_event(
                tracing::Level::INFO,
                ProcessKind::Desktop,
                ObservabilityEvent {
                    event: "chat.dispatch.sent",
                    component: "tauri.commands",
                    org_id: None,
                    workspace_id: None,
                    correlation_id: Some(&correlation_id),
                    session_id: Some(&session_id),
                    run_id: None,
                    message_id: None,
                    provider_id: None,
                    model_id: None,
                    status: Some("ok"),
                    error_code: None,
                    detail: Some("prompt_async accepted"),
                },
            );
            Ok(())
        }
        Err(err) => {
            emit_event(
                tracing::Level::ERROR,
                ProcessKind::Desktop,
                ObservabilityEvent {
                    event: "chat.dispatch.failed",
                    component: "tauri.commands",
                    org_id: None,
                    workspace_id: None,
                    correlation_id: Some(&correlation_id),
                    session_id: Some(&session_id),
                    run_id: None,
                    message_id: None,
                    provider_id: None,
                    model_id: None,
                    status: Some("failed"),
                    error_code: Some("ENGINE_DISPATCH_FAILED"),
                    detail: Some("prompt_async request failed"),
                },
            );
            Err(err)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct QueuedMessageView {
    pub id: String,
    pub content: String,
    pub attachments: Vec<crate::state::QueuedAttachment>,
    pub created_at_ms: u64,
}

#[tauri::command]
pub async fn queue_message(
    state: State<'_, AppState>,
    session_id: String,
    content: String,
    attachments: Option<Vec<FileAttachmentInput>>,
) -> Result<QueuedMessageView> {
    if session_id.trim().is_empty() {
        return Err(TandemError::InvalidConfig(
            "session_id cannot be empty".to_string(),
        ));
    }
    let item = crate::state::QueuedMessage {
        id: Uuid::new_v4().to_string(),
        content,
        attachments: attachment_inputs_to_queued(attachments),
        created_at_ms: logs::now_ms(),
    };
    let mut guard = state.message_queue.lock().await;
    let queue = guard.entry(session_id).or_insert_with(Vec::new);
    queue.push(item.clone());

    Ok(QueuedMessageView {
        id: item.id,
        content: item.content,
        attachments: item.attachments,
        created_at_ms: item.created_at_ms,
    })
}

#[tauri::command]
pub async fn queue_list(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<QueuedMessageView>> {
    let guard = state.message_queue.lock().await;
    let items = guard.get(&session_id).cloned().unwrap_or_default();
    Ok(items
        .into_iter()
        .map(|m| QueuedMessageView {
            id: m.id,
            content: m.content,
            attachments: m.attachments,
            created_at_ms: m.created_at_ms,
        })
        .collect())
}

#[tauri::command]
pub async fn queue_remove(
    state: State<'_, AppState>,
    session_id: String,
    item_id: String,
) -> Result<bool> {
    let mut guard = state.message_queue.lock().await;
    let Some(queue) = guard.get_mut(&session_id) else {
        return Ok(false);
    };
    let original_len = queue.len();
    queue.retain(|q| q.id != item_id);
    Ok(queue.len() != original_len)
}

#[tauri::command]
pub async fn queue_send_next(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<bool> {
    queue_send_next_internal(&app, &state, &session_id).await
}

#[tauri::command]
pub async fn queue_send_all(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<u32> {
    let mut sent = 0u32;
    loop {
        let has_more = queue_send_next_internal(&app, &state, &session_id).await?;
        if !has_more {
            break;
        }
        sent += 1;
    }
    Ok(sent)
}

async fn queue_send_next_internal(
    app: &AppHandle,
    state: &AppState,
    session_id: &str,
) -> Result<bool> {
    let config_snapshot = { state.providers_config.read().unwrap().clone() };
    let model_spec =
        resolve_required_model_spec(&config_snapshot, None, None, "Queued message dispatch")?;

    let next = {
        let mut guard = state.message_queue.lock().await;
        let Some(queue) = guard.get_mut(session_id) else {
            return Ok(false);
        };
        if queue.is_empty() {
            return Ok(false);
        }
        queue.remove(0)
    };

    let attachments = queued_to_attachment_inputs(next.attachments.clone());
    let send_res = send_message_and_start_run_internal(
        app,
        state,
        session_id.to_string(),
        next.content.clone(),
        attachments,
        None,
        None,
        Some(model_spec.model_id),
        Some(model_spec.provider_id),
        true,
    )
    .await;

    if let Err(e) = send_res {
        let mut guard = state.message_queue.lock().await;
        let queue = guard.entry(session_id.to_string()).or_insert_with(Vec::new);
        queue.insert(0, next);
        return Err(e);
    }

    Ok(true)
}

/// Cancel ongoing generation
#[tauri::command]
pub async fn cancel_generation(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.sidecar.cancel_generation(&session_id).await
}
