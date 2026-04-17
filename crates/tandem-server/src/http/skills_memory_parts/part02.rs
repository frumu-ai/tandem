
fn memory_promote_metadata(
    metadata: Option<&Value>,
    request: &MemoryPromoteRequest,
    promoted_at_ms: u64,
) -> Option<Value> {
    let mut obj = metadata
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    obj.insert(
        "promotion".to_string(),
        json!({
            "promoted_at_ms": promoted_at_ms,
            "promote_run_id": request.run_id,
            "source_memory_id": request.source_memory_id,
            "from_tier": request.from_tier,
            "to_tier": request.to_tier,
            "reason": request.reason,
            "review": {
                "required": request.review.required,
                "reviewer_id": request.review.reviewer_id,
                "approval_id": request.review.approval_id,
            },
        }),
    );
    Some(Value::Object(obj))
}

fn memory_promote_provenance(
    provenance: Option<&Value>,
    request: &MemoryPromoteRequest,
    partition_key: &str,
    promoted_at_ms: u64,
    tenant_context: &TenantContext,
) -> Value {
    let mut obj = provenance
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    obj.insert(
        "promotion".to_string(),
        json!({
            "promoted_at_ms": promoted_at_ms,
            "promote_run_id": request.run_id,
            "source_memory_id": request.source_memory_id,
            "partition_key": partition_key,
            "to_tier": request.to_tier,
            "reviewer_id": request.review.reviewer_id,
            "approval_id": request.review.approval_id,
            "tenant_context": tenant_context,
        }),
    );
    Value::Object(obj)
}

fn memory_linkage(record: &GlobalMemoryRecord) -> Value {
    memory_linkage_from_parts(
        &record.run_id,
        record.project_tag.as_deref(),
        record.metadata.as_ref(),
        record.provenance.as_ref(),
    )
}

fn memory_linkage_from_parts(
    run_id: &str,
    project_id: Option<&str>,
    metadata: Option<&Value>,
    provenance: Option<&Value>,
) -> Value {
    let artifact_refs = memory_artifact_refs(metadata);
    json!({
        "run_id": run_id,
        "project_id": project_id,
        "origin_event_type": provenance
            .and_then(|row| row.get("origin_event_type"))
            .and_then(Value::as_str),
        "origin_run_id": provenance
            .and_then(|row| row.get("origin_run_id"))
            .and_then(Value::as_str)
            .or(Some(run_id)),
        "origin_session_id": provenance
            .and_then(|row| row.get("origin_session_id"))
            .and_then(Value::as_str),
        "origin_message_id": provenance
            .and_then(|row| row.get("origin_message_id"))
            .and_then(Value::as_str),
        "partition_key": provenance
            .and_then(|row| row.get("partition_key"))
            .and_then(Value::as_str),
        "promote_run_id": provenance
            .and_then(|row| row.get("promotion"))
            .and_then(|row| row.get("promote_run_id"))
            .and_then(Value::as_str),
        "approval_id": provenance
            .and_then(|row| row.get("promotion"))
            .and_then(|row| row.get("approval_id"))
            .and_then(Value::as_str),
        "artifact_refs": artifact_refs,
    })
}

fn memory_kind_label(source_type: &str) -> &str {
    match source_type {
        "solution_capsule" => "solution_capsule",
        "note" => "note",
        "fact" => "fact",
        other => other,
    }
}

fn memory_linkage_detail(linkage: &Value) -> String {
    let origin_run_id = linkage
        .get("origin_run_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let project_id = linkage
        .get("project_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let promote_run_id = linkage
        .get("promote_run_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    format!(
        " origin_run_id={} project_id={} promote_run_id={}",
        origin_run_id, project_id, promote_run_id
    )
}

fn memory_kind_for_request(kind: tandem_memory::MemoryContentKind) -> &'static str {
    match kind {
        tandem_memory::MemoryContentKind::SolutionCapsule => "solution_capsule",
        tandem_memory::MemoryContentKind::Note => "note",
        tandem_memory::MemoryContentKind::Fact => "fact",
    }
}

fn memory_tier_for_visibility(visibility: &str) -> tandem_memory::GovernedMemoryTier {
    if visibility.eq_ignore_ascii_case("shared") {
        tandem_memory::GovernedMemoryTier::Project
    } else {
        tandem_memory::GovernedMemoryTier::Session
    }
}

fn memory_classification_label(metadata: Option<&Value>) -> &str {
    metadata
        .and_then(|row| row.get("classification"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("internal")
}

pub(super) fn scrub_content(input: &str) -> ScrubReport {
    let mut redactions = 0u32;
    let mut blocked = false;
    let lower = input.to_lowercase();
    let redact_markers = [
        "api_key",
        "secret=",
        "authorization: bearer",
        "x-api-key",
        "token=",
    ];
    for marker in redact_markers {
        if lower.contains(marker) {
            redactions = redactions.saturating_add(1);
        }
    }
    let block_markers = [
        "-----begin private key-----",
        "aws_secret_access_key",
        "sk-ant-",
        "ghp_",
    ];
    for marker in block_markers {
        if lower.contains(marker) {
            blocked = true;
            break;
        }
    }
    if blocked {
        ScrubReport {
            status: ScrubStatus::Blocked,
            redactions,
            block_reason: Some("sensitive secret marker detected".to_string()),
        }
    } else if redactions > 0 {
        ScrubReport {
            status: ScrubStatus::Redacted,
            redactions,
            block_reason: None,
        }
    } else {
        ScrubReport {
            status: ScrubStatus::Passed,
            redactions: 0,
            block_reason: None,
        }
    }
}

pub(super) fn scrub_content_for_memory(input: &str) -> (String, ScrubReport) {
    let mut scrubbed = input.to_string();
    let mut redactions = 0u32;
    let mut blocked = false;
    let redact_patterns = [
        r"(?i)authorization:\s*bearer\s+[a-z0-9\.\-_]+",
        r"(?i)(api[_-]?key|token|secret)\s*[:=]\s*[a-z0-9\-_]{8,}",
        r"(?i)x-api-key\s*:\s*[a-z0-9\-_]{8,}",
        r"(?i)sk-[a-z0-9]{12,}",
        r"(?i)ghp_[a-z0-9]{12,}",
    ];
    for pattern in redact_patterns {
        if let Ok(re) = Regex::new(pattern) {
            let matches = re.find_iter(&scrubbed).count() as u32;
            if matches > 0 {
                redactions = redactions.saturating_add(matches);
                scrubbed = re.replace_all(&scrubbed, "[REDACTED]").to_string();
            }
        }
    }
    let block_markers = [
        "-----begin private key-----",
        "aws_secret_access_key",
        "-----begin rsa private key-----",
    ];
    let lowered = input.to_lowercase();
    for marker in block_markers {
        if lowered.contains(marker) {
            blocked = true;
            break;
        }
    }
    if blocked {
        (
            String::new(),
            ScrubReport {
                status: ScrubStatus::Blocked,
                redactions,
                block_reason: Some("sensitive secret marker detected".to_string()),
            },
        )
    } else if redactions > 0 {
        (
            scrubbed,
            ScrubReport {
                status: ScrubStatus::Redacted,
                redactions,
                block_reason: None,
            },
        )
    } else {
        (
            scrubbed,
            ScrubReport {
                status: ScrubStatus::Passed,
                redactions: 0,
                block_reason: None,
            },
        )
    }
}

pub(super) fn hash_text(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) async fn append_memory_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    mut event: crate::MemoryAuditEvent,
) -> Result<(), StatusCode> {
    event.tenant_context = tenant_context.clone();
    if let Some(parent) = state.memory_audit_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let line = serde_json::to_string(&event).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.memory_audit_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, line.as_bytes())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::io::AsyncWriteExt::write_all(&mut file, b"\n")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    file.sync_data()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut audit = state.memory_audit_log.write().await;
    audit.push(event);
    Ok(())
}

async fn load_memory_audit_events(path: &std::path::Path) -> Vec<crate::MemoryAuditEvent> {
    let Ok(content) = tokio::fs::read_to_string(path).await else {
        return Vec::new();
    };

    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<crate::MemoryAuditEvent>(trimmed).ok()
        })
        .collect()
}

#[derive(Debug, Clone)]
pub(super) struct RunMemoryContext {
    run_id: String,
    user_id: String,
    started_at_ms: u64,
    host_tag: Option<String>,
    tenant_context: TenantContext,
}

pub(super) async fn open_global_memory_db() -> Option<MemoryDatabase> {
    let paths = tandem_core::resolve_shared_paths().ok()?;
    if let Some(parent) = paths.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    MemoryDatabase::new(&paths.memory_db_path).await.ok()
}

pub(super) async fn open_memory_manager() -> Option<tandem_memory::MemoryManager> {
    let paths = tandem_core::resolve_shared_paths().ok()?;
    if let Some(parent) = paths.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tandem_memory::MemoryManager::new(&paths.memory_db_path)
        .await
        .ok()
}

pub(super) fn event_run_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("runID")
        .or_else(|| event.properties.get("run_id"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub(super) fn event_session_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("sessionID")
        .or_else(|| event.properties.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub(super) fn summarize_value(value: &Value, limit: usize) -> String {
    let text = if value.is_string() {
        value.as_str().unwrap_or_default().to_string()
    } else {
        value.to_string()
    };
    truncate_text(&text, limit)
}

pub(super) async fn persist_global_memory_record(
    state: &AppState,
    db: &MemoryDatabase,
    mut record: GlobalMemoryRecord,
) {
    let tenant_context = record_tenant_context(&record);
    publish_tenant_event(
        state,
        &tenant_context,
        "memory.write.attempted",
        json!({
            "runID": record.run_id,
            "sourceType": record.source_type,
            "sessionID": record.session_id,
            "messageID": record.message_id,
        }),
    );
    let (scrubbed, scrub) = scrub_content_for_memory(&record.content);
    if scrub.status == ScrubStatus::Blocked || scrubbed.trim().is_empty() {
        publish_tenant_event(
            state,
            &tenant_context,
            "memory.write.skipped",
            json!({
                "runID": record.run_id,
                "sourceType": record.source_type,
                "reason": scrub.block_reason.unwrap_or_else(|| "scrub_blocked".to_string()),
                "sessionID": record.session_id,
                "messageID": record.message_id,
            }),
        );
        return;
    }
    record.content = scrubbed;
    record.redaction_count = scrub.redactions;
    record.redaction_status = match scrub.status {
        ScrubStatus::Passed => "passed".to_string(),
        ScrubStatus::Redacted => "redacted".to_string(),
        ScrubStatus::Blocked => "blocked".to_string(),
    };
    record.content_hash = hash_text(&record.content);
    match db.put_global_memory_record(&record).await {
        Ok(write) => {
            let event_name = if write.deduped {
                "memory.write.skipped"
            } else {
                "memory.write.succeeded"
            };
            publish_tenant_event(
                state,
                &tenant_context,
                event_name,
                json!({
                    "runID": record.run_id,
                    "memoryID": write.id,
                    "sourceType": record.source_type,
                    "deduped": write.deduped,
                    "redactionStatus": record.redaction_status,
                    "redactionCount": record.redaction_count,
                    "sessionID": record.session_id,
                    "messageID": record.message_id,
                }),
            );
        }
        Err(err) => {
            publish_tenant_event(
                state,
                &tenant_context,
                "memory.write.skipped",
                json!({
                    "runID": record.run_id,
                    "sourceType": record.source_type,
                    "reason": format!("db_error:{err}"),
                    "sessionID": record.session_id,
                    "messageID": record.message_id,
                }),
            );
        }
    }
}

pub(super) async fn ingest_run_messages(
    state: &AppState,
    db: &MemoryDatabase,
    session_id: &str,
    ctx: &RunMemoryContext,
) {
    let Some(session) = state.storage.get_session(session_id).await else {
        return;
    };
    for message in session.messages {
        let created_ms = message.created_at.timestamp_millis() as u64;
        if created_ms + 1_000 < ctx.started_at_ms {
            continue;
        }
        for part in message.parts {
            match (message.role.clone(), part) {
                (MessageRole::User, MessagePart::Text { text }) => {
                    let now = crate::now_ms();
                    persist_global_memory_record(
                        state,
                        db,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "user_message".to_string(),
                            content: text,
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: None,
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: Some(json!({"role": "user"})),
                            provenance: Some(json!({"origin_event_type": "session.run.finished", "origin_message_id": message.id, "origin_session_id": session_id})),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: None,
                        },
                    )
                    .await;
                }
                (MessageRole::Assistant, MessagePart::Text { text }) => {
                    let now = crate::now_ms();
                    persist_global_memory_record(
                        state,
                        db,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "assistant_final".to_string(),
                            content: text,
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: None,
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: Some(json!({"role": "assistant"})),
                            provenance: Some(json!({"origin_event_type": "session.run.finished", "origin_message_id": message.id, "origin_session_id": session_id})),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: None,
                        },
                    )
                    .await;
                }
                (
                    MessageRole::Assistant | MessageRole::Tool,
                    MessagePart::ToolInvocation {
                        tool,
                        args,
                        result,
                        error,
                    },
                ) => {
                    let now = crate::now_ms();
                    let tool_input = summarize_value(&args, 1200);
                    persist_global_memory_record(
                        state,
                        db,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "tool_input".to_string(),
                            content: format!("tool={} args={}", tool, tool_input),
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: Some(tool.clone()),
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: None,
                            provenance: Some(json!({
                                "origin_event_type": "session.run.finished",
                                "tenant_context": ctx.tenant_context,
                            })),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: Some(now + 30 * 24 * 60 * 60 * 1000),
                        },
                    )
                    .await;
                    let tool_output = result
                        .as_ref()
                        .map(|v| summarize_value(v, 1500))
                        .or(error)
                        .unwrap_or_default();
                    if !tool_output.trim().is_empty() {
                        let now = crate::now_ms();
                        persist_global_memory_record(
                            state,
                            db,
                            GlobalMemoryRecord {
                                id: Uuid::new_v4().to_string(),
                                user_id: ctx.user_id.clone(),
                                source_type: "tool_output".to_string(),
                                content: format!("tool={} output={}", tool, tool_output),
                                content_hash: String::new(),
                                run_id: ctx.run_id.clone(),
                                session_id: Some(session_id.to_string()),
                                message_id: Some(message.id.clone()),
                                tool_name: Some(tool),
                                project_tag: session.project_id.clone(),
                                channel_tag: None,
                                host_tag: ctx.host_tag.clone(),
                                metadata: None,
                                provenance: Some(
                                    json!({"origin_event_type": "session.run.finished"}),
                                ),
                                redaction_status: "passed".to_string(),
                                redaction_count: 0,
                                visibility: "private".to_string(),
                                demoted: false,
                                score_boost: 0.0,
                                created_at_ms: now,
                                updated_at_ms: now,
                                expires_at_ms: Some(now + 30 * 24 * 60 * 60 * 1000),
                            },
                        )
                        .await;
                    }
                }
                _ => {}
            }
        }
    }
}

pub(super) async fn ingest_event_memory_records(
    state: &AppState,
    db: &MemoryDatabase,
    event: &EngineEvent,
    ctx_by_session: &HashMap<String, RunMemoryContext>,
) {
    let session_id = event_session_id(event);
    let session_ctx = session_id
        .as_ref()
        .and_then(|sid| ctx_by_session.get(sid))
        .cloned();
    let run_id = event_run_id(event)
        .or_else(|| session_ctx.as_ref().map(|c| c.run_id.clone()))
        .unwrap_or_else(|| "unknown".to_string());
    let user_id = session_ctx
        .as_ref()
        .map(|c| c.user_id.clone())
        .unwrap_or_else(|| "default".to_string());
    let host_tag = session_ctx.as_ref().and_then(|c| c.host_tag.clone());
    let tenant_context = event_tenant_context(event)
        .or_else(|| session_ctx.as_ref().map(|c| c.tenant_context.clone()))
        .unwrap_or_default();
    let (source_type, content, ttl_ms): (&str, String, Option<u64>) =
        match event.event_type.as_str() {
            "permission.asked" => (
                "approval_request",
                format!(
                    "permission requested tool={} query={}",
                    event
                        .properties
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    event
                        .properties
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                ),
                Some(14 * 24 * 60 * 60 * 1000),
            ),
            "permission.replied" => (
                "approval_decision",
                format!(
                    "permission reply requestID={} reply={}",
                    event
                        .properties
                        .get("requestID")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event
                        .properties
                        .get("reply")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                Some(14 * 24 * 60 * 60 * 1000),
            ),
            "mcp.auth.required" | "mcp.auth.pending" => (
                "auth_challenge",
                format!(
                    "mcp auth tool={} server={} status={} message={}",
                    event
                        .properties
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event
                        .properties
                        .get("server")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event.event_type,
                    event
                        .properties
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                Some(7 * 24 * 60 * 60 * 1000),
            ),
            "todo.updated" => (
                "plan_todos",
                format!(
                    "todo updated: {}",
                    summarize_value(event.properties.get("todos").unwrap_or(&Value::Null), 1200)
                ),
                Some(60 * 24 * 60 * 60 * 1000),
            ),
            "question.asked" => (
                "question_prompt",
                format!(
                    "question asked: {}",
                    summarize_value(
                        event.properties.get("questions").unwrap_or(&Value::Null),
                        1200
                    )
                ),
                Some(60 * 24 * 60 * 60 * 1000),
            ),
            _ => return,
        };
    let now = crate::now_ms();
    persist_global_memory_record(
        state,
        db,
        GlobalMemoryRecord {
            id: Uuid::new_v4().to_string(),
            user_id,
            source_type: source_type.to_string(),
            content,
            content_hash: String::new(),
            run_id,
            session_id,
            message_id: event
                .properties
                .get("messageID")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            tool_name: event
                .properties
                .get("tool")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            project_tag: None,
            channel_tag: event
                .properties
                .get("channel")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            host_tag,
            metadata: None,
            provenance: Some(json!({
                "origin_event_type": event.event_type,
                "tenant_context": tenant_context,
            })),
            redaction_status: "passed".to_string(),
            redaction_count: 0,
            visibility: "private".to_string(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: ttl_ms.map(|ttl| now + ttl),
        },
    )
    .await;
}

pub(super) async fn run_global_memory_ingestor(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("global memory ingestor: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    let Some(db) = open_global_memory_db().await else {
        tracing::warn!("global memory ingestor disabled: could not open memory database");
        return;
    };
    let mut by_session: HashMap<String, RunMemoryContext> = HashMap::new();
    loop {
        match rx.recv().await {
            Ok(event) => match event.event_type.as_str() {
                "session.run.started" => {
                    let session_id = event_session_id(&event);
                    let run_id = event_run_id(&event);
                    if let (Some(session_id), Some(run_id)) = (session_id, run_id) {
                        let started_at_ms = event
                            .properties
                            .get("startedAtMs")
                            .and_then(|v| v.as_u64())
                            .unwrap_or_else(crate::now_ms);
                        let user_id = event
                            .properties
                            .get("clientID")
                            .and_then(|v| v.as_str())
                            .filter(|v| !v.trim().is_empty())
                            .unwrap_or("default")
                            .to_string();
                        let host_tag = event
                            .properties
                            .get("environment")
                            .and_then(|v| v.get("os"))
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string);
                        let tenant_context = event_tenant_context(&event).unwrap_or_default();
                        by_session.insert(
                            session_id,
                            RunMemoryContext {
                                run_id,
                                user_id,
                                started_at_ms,
                                host_tag,
                                tenant_context,
                            },
                        );
                    }
                }
                "session.run.finished" => {
                    if let Some(session_id) = event_session_id(&event) {
                        if let Some(ctx) = by_session.remove(&session_id) {
                            ingest_run_messages(&state, &db, &session_id, &ctx).await;
                        }
                    }
                }
                "permission.asked" | "permission.replied" | "mcp.auth.required"
                | "mcp.auth.pending" | "todo.updated" | "question.asked" => {
                    ingest_event_memory_records(&state, &db, &event, &by_session).await;
                }
                _ => {}
            },
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub(super) async fn memory_put(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Json(input): Json<MemoryPutInput>,
) -> Result<Json<MemoryPutResponse>, StatusCode> {
    let response =
        memory_put_impl(&state, &tenant_context, input.request, input.capability).await?;
    Ok(Json(response))
}

pub(super) async fn memory_put_impl(
    state: &AppState,
    tenant_context: &TenantContext,
    request: MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPutResponse, StatusCode> {
    let capability =
        validate_memory_put_capability_with_guardrail(state, tenant_context, &request, capability)
            .await?;
    if !capability
        .memory
        .write_tiers
        .contains(&request.partition.tier)
    {
        emit_blocked_memory_put_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            "write tier not allowed by capability",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    let id = Uuid::new_v4().to_string();
    let partition_key = request.partition.key();
    let kind = memory_kind_for_request(request.kind.clone());
    let now = crate::now_ms();
    let audit_id = Uuid::new_v4().to_string();
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let artifact_refs = request.artifact_refs.clone();
    let artifact_ref_labels = artifact_refs
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let source_type = match request.kind {
        tandem_memory::MemoryContentKind::SolutionCapsule => "solution_capsule",
        tandem_memory::MemoryContentKind::Note => "note",
        tandem_memory::MemoryContentKind::Fact => "fact",
    }
    .to_string();
    let user_id = capability.subject.clone();
    let record = GlobalMemoryRecord {
        id: id.clone(),
        user_id,
        source_type,
        content: request.content.clone(),
        content_hash: String::new(),
        run_id: request.run_id.clone(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some(request.partition.project_id.clone()),
        channel_tag: None,
        host_tag: None,
        metadata: memory_metadata_with_storage_fields(
            request.metadata.clone(),
            &artifact_refs,
            request.classification,
        ),
        provenance: Some(memory_put_provenance(
            &request,
            &partition_key,
            &artifact_refs,
            tenant_context,
        )),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: now,
        updated_at_ms: now,
        expires_at_ms: None,
    };
    let memory_linkage_value = memory_linkage_from_parts(
        &request.run_id,
        Some(&request.partition.project_id),
        record.metadata.as_ref(),
        record.provenance.as_ref(),
    );
    let put_detail = format!(
        "kind={} classification={} artifact_refs={} visibility=private tier={} partition_key={}{}",
        kind,
        memory_classification_label(record.metadata.as_ref()),
        artifact_ref_labels,
        request.partition.tier,
        partition_key,
        memory_linkage_detail(&memory_linkage_value)
    );
    persist_global_memory_record(&state, &db, record).await;
    append_memory_audit(
        &state,
        tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_put".to_string(),
            run_id: request.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: Some(id.clone()),
            source_memory_id: None,
            to_tier: Some(request.partition.tier),
            partition_key: partition_key.clone(),
            actor: capability.subject,
            status: "ok".to_string(),
            detail: Some(put_detail),
            created_at_ms: now,
        },
    )
    .await?;
    publish_tenant_event(
        state,
        tenant_context,
        "memory.put",
        json!({
            "runID": request.run_id,
            "memoryID": id,
            "kind": kind,
            "classification": request.classification,
            "artifactRefs": artifact_refs,
            "visibility": "private",
            "tier": request.partition.tier,
            "partitionKey": partition_key,
            "linkage": memory_linkage_value.clone(),
            "auditID": audit_id,
        }),
    );
    publish_tenant_event(
        state,
        tenant_context,
        "memory.updated",
        json!({
            "memoryID": id,
            "runID": request.run_id,
            "action": "put",
            "kind": kind,
            "classification": request.classification,
            "artifactRefs": artifact_refs,
            "visibility": "private",
            "tier": request.partition.tier,
            "partitionKey": partition_key,
            "linkage": memory_linkage_value,
            "auditID": audit_id,
        }),
    );
    Ok(MemoryPutResponse {
        id,
        stored: true,
        tier: request.partition.tier,
        partition_key,
        audit_id,
    })
}

pub(super) async fn memory_promote(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Json(input): Json<MemoryPromoteInput>,
) -> Result<Json<MemoryPromoteResponse>, StatusCode> {
    let response =
        memory_promote_impl(&state, &tenant_context, input.request, input.capability).await?;
    Ok(Json(response))
}

pub(super) async fn memory_promote_impl(
    state: &AppState,
    tenant_context: &TenantContext,
    request: MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPromoteResponse, StatusCode> {
    let source_memory_id = request.source_memory_id.clone();
    let capability = validate_memory_promote_capability_with_guardrail(
        state,
        tenant_context,
        &request,
        capability,
    )
    .await?;
    if !capability.memory.promote_targets.contains(&request.to_tier) {
        emit_blocked_memory_promote_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            "promotion target not allowed by capability",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    if capability.memory.require_review_for_promote
        && (request.review.approval_id.is_none() || request.review.reviewer_id.is_none())
    {
        emit_blocked_memory_promote_guardrail(
            state,
            tenant_context,
            &request,
            capability.subject.clone(),
            "review approval required for promote",
        )
        .await?;
        return Err(StatusCode::FORBIDDEN);
    }
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let source = db
        .get_global_memory(&request.source_memory_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(source) = source else {
        let scrub_report = ScrubReport {
            status: ScrubStatus::Blocked,
            redactions: 0,
            block_reason: Some("source memory missing or previously blocked".to_string()),
        };
        let audit_id = Uuid::new_v4().to_string();
        let partition_key = format!(
            "{}/{}/{}/{}",
            request.partition.org_id,
            request.partition.workspace_id,
            request.partition.project_id,
            request.to_tier
        );
        let linkage = json!({
            "run_id": request.run_id,
            "project_id": request.partition.project_id,
            "origin_event_type": Value::Null,
            "origin_run_id": request.run_id,
            "origin_session_id": Value::Null,
            "origin_message_id": Value::Null,
            "partition_key": partition_key,
            "promote_run_id": Value::Null,
            "approval_id": request.review.approval_id,
            "artifact_refs": [],
        });
        append_memory_audit(
            &state,
            tenant_context,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                tenant_context: tenant_context.clone(),
                memory_id: None,
                source_memory_id: Some(source_memory_id.clone()),
                to_tier: Some(request.to_tier),
                partition_key: partition_key.clone(),
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: scrub_report
                    .block_reason
                    .as_ref()
                    .map(|detail| format!("{detail}{}", memory_linkage_detail(&linkage))),
                created_at_ms: crate::now_ms(),
            },
        )
        .await?;
        publish_tenant_event(
            state,
            tenant_context,
            "memory.promote",
            json!({
                "runID": request.run_id,
                "sourceMemoryID": source_memory_id,
                "toTier": request.to_tier,
                "partitionKey": partition_key,
                "status": "blocked",
                "kind": Value::Null,
                "classification": Value::Null,
                "artifactRefs": [],
                "visibility": Value::Null,
                "scrubStatus": scrub_report.status,
                "linkage": linkage,
                "detail": scrub_report.block_reason.clone(),
                "auditID": audit_id,
            }),
        );
        return Ok(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
        });
    };
    let scrub_report = scrub_content(&source.content);
    let audit_id = Uuid::new_v4().to_string();
    let now = crate::now_ms();
    let partition_key = format!(
        "{}/{}/{}/{}",
        request.partition.org_id,
        request.partition.workspace_id,
        request.partition.project_id,
        request.to_tier
    );
    let linkage = memory_linkage(&source);
    if scrub_report.status == ScrubStatus::Blocked {
        append_memory_audit(
            &state,
            tenant_context,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                tenant_context: tenant_context.clone(),
                memory_id: None,
                source_memory_id: Some(source_memory_id.clone()),
                to_tier: Some(request.to_tier),
                partition_key: partition_key.clone(),
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: scrub_report
                    .block_reason
                    .as_ref()
                    .map(|detail| format!("{detail}{}", memory_linkage_detail(&linkage))),
                created_at_ms: now,
            },
        )
        .await?;
        publish_tenant_event(
            state,
            tenant_context,
            "memory.promote",
            json!({
                "runID": request.run_id,
                "sourceMemoryID": source_memory_id,
                "toTier": request.to_tier,
                "partitionKey": partition_key,
                "status": "blocked",
                "kind": memory_kind_label(&source.source_type),
                "classification": memory_classification_label(source.metadata.as_ref()),
                "artifactRefs": memory_artifact_refs(source.metadata.as_ref()),
                "visibility": source.visibility,
                "scrubStatus": scrub_report.status,
                "linkage": linkage,
                "detail": scrub_report.block_reason.clone(),
                "auditID": audit_id,
            }),
        );
        return Ok(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
        });
    }
    let new_id = source.id.clone();
    let next_metadata = memory_promote_metadata(source.metadata.as_ref(), &request, now);
    let next_provenance = memory_promote_provenance(
        source.provenance.as_ref(),
        &request,
        &partition_key,
        now,
        tenant_context,
    );
    let classification = memory_classification_label(next_metadata.as_ref());
    let artifact_refs = memory_artifact_refs(next_metadata.as_ref());
    let artifact_ref_labels = artifact_refs
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let kind = memory_kind_label(&source.source_type);
    let promote_detail = format!(
        "kind={} classification={} artifact_refs={} visibility=shared tier={} partition_key={} source_memory_id={} approval_id={}{}",
        kind,
        classification,
        artifact_ref_labels,
        request.to_tier,
        partition_key,
        source_memory_id,
        request.review.approval_id.clone().unwrap_or_default(),
        memory_linkage_detail(&memory_linkage_from_parts(
            &source.run_id,
            source.project_tag.as_deref(),
            next_metadata.as_ref(),
            Some(&next_provenance),
        ))
    );
    db.update_global_memory_context(
        &new_id,
        "shared",
        false,
        next_metadata.as_ref(),
        Some(&next_provenance),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    append_memory_audit(
        &state,
        tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_promote".to_string(),
            run_id: request.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: Some(new_id.clone()),
            source_memory_id: Some(source_memory_id.clone()),
            to_tier: Some(request.to_tier),
            partition_key: format!(
                "{}/{}/{}/{}",
                request.partition.org_id,
                request.partition.workspace_id,
                request.partition.project_id,
                request.to_tier
            ),
            actor: capability.subject,
            status: "ok".to_string(),
            detail: Some(promote_detail),
            created_at_ms: now,
        },
    )
    .await?;
    publish_tenant_event(
        state,
        tenant_context,
        "memory.promote",
        json!({
            "runID": request.run_id,
            "sourceMemoryID": source_memory_id,
            "memoryID": new_id,
            "kind": kind,
            "classification": classification,
            "artifactRefs": artifact_refs,
            "visibility": "shared",
            "toTier": request.to_tier,
            "partitionKey": partition_key,
            "linkage": memory_linkage_from_parts(
                &source.run_id,
                source.project_tag.as_deref(),
                next_metadata.as_ref(),
                Some(&next_provenance),
            ),
            "approvalID": request.review.approval_id,
            "auditID": audit_id,
            "scrubStatus": scrub_report.status,
        }),
    );
    publish_tenant_event(
        state,
        tenant_context,
        "memory.updated",
        json!({
            "memoryID": new_id,
            "runID": request.run_id,
            "action": "promote",
            "kind": kind,
            "classification": classification,
            "artifactRefs": artifact_refs,
            "visibility": "shared",
            "tier": request.to_tier,
            "partitionKey": partition_key,
            "linkage": memory_linkage_from_parts(
                &source.run_id,
                source.project_tag.as_deref(),
                next_metadata.as_ref(),
                Some(&next_provenance),
            ),
            "sourceMemoryID": source_memory_id,
            "approvalID": request.review.approval_id,
            "auditID": audit_id,
        }),
    );
    Ok(MemoryPromoteResponse {
        promoted: true,
        new_memory_id: Some(new_id),
        to_tier: request.to_tier,
        scrub_report,
        audit_id,
    })
}

pub(super) async fn memory_search(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Json(input): Json<MemorySearchInput>,
) -> Result<Json<MemorySearchResponse>, StatusCode> {
    let request = input.request;
    let capability = validate_memory_search_capability_with_guardrail(
        &state,
        &tenant_context,
        &request,
        input.capability,
    )
    .await?;
    let requested_scopes = if request.read_scopes.is_empty() {
        capability.memory.read_tiers.clone()
    } else {
        request.read_scopes.clone()
    };
    let mut scopes_used = Vec::new();
    let mut blocked_scopes = Vec::new();
    for scope in &requested_scopes {
        if capability.memory.read_tiers.contains(scope) {
            scopes_used.push(scope.clone());
        } else {
            blocked_scopes.push(scope.clone());
        }
    }
    let allow_private_results = scopes_used
        .iter()
        .any(|scope| matches!(scope, tandem_memory::GovernedMemoryTier::Session));
    let limit = request.limit.unwrap_or(8).clamp(1, 100);
    let hits = if scopes_used.is_empty() {
        Vec::new()
    } else {
        let db = open_global_memory_db()
            .await
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        db.search_global_memory(
            &capability.subject,
            &request.query,
            limit,
            Some(&request.partition.project_id),
            None,
            None,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .filter(|hit| allow_private_results || hit.record.visibility.eq_ignore_ascii_case("shared"))
        .collect::<Vec<_>>()
    };
    let results = hits
        .into_iter()
        .map(|hit| {
            json!({
                "id": hit.record.id,
                "tier": memory_tier_for_visibility(&hit.record.visibility),
                "classification": memory_classification_label(hit.record.metadata.as_ref()),
                "kind": memory_kind_label(&hit.record.source_type),
                "source_type": hit.record.source_type,
                "created_at_ms": hit.record.created_at_ms,
                "content": hit.record.content,
                "score": hit.score,
                "run_id": hit.record.run_id,
                "visibility": hit.record.visibility,
                "artifact_refs": memory_artifact_refs(hit.record.metadata.as_ref()),
                "linkage": memory_linkage(&hit.record),
                "metadata": hit.record.metadata,
                "provenance": hit.record.provenance,
            })
        })
        .collect::<Vec<_>>();
    let result_ids = results
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let result_kinds = results
        .iter()
        .filter_map(|row| row.get("kind").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let linkage = json!({
        "run_id": request.run_id,
        "project_id": request.partition.project_id,
        "origin_event_type": "memory.search",
        "origin_run_id": request.run_id,
        "origin_session_id": Value::Null,
        "origin_message_id": Value::Null,
        "partition_key": request.partition.key(),
        "promote_run_id": Value::Null,
        "approval_id": Value::Null,
        "artifact_refs": [],
    });
    let audit_id = Uuid::new_v4().to_string();
    let now = crate::now_ms();
    let search_status = if scopes_used.is_empty() && !blocked_scopes.is_empty() {
        "blocked"
    } else {
        "ok"
    };
    let search_detail = format!(
        "query={} result_count={} result_ids={} result_kinds={} requested_scopes={} scopes_used={} blocked_scopes={}{}",
        request.query,
        results.len(),
        result_ids.join(","),
        result_kinds.join(","),
        requested_scopes
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>()
            .join(","),
        scopes_used
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>()
            .join(","),
        blocked_scopes
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>()
            .join(","),
        memory_linkage_detail(&linkage)
    );
    append_memory_audit(
        &state,
        &tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_search".to_string(),
            run_id: request.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: None,
            source_memory_id: None,
            to_tier: None,
            partition_key: request.partition.key(),
            actor: capability.subject,
            status: search_status.to_string(),
            detail: Some(search_detail),
            created_at_ms: now,
        },
    )
    .await?;
    publish_tenant_event(
        &state,
        &tenant_context,
        "memory.search",
        json!({
            "runID": request.run_id,
            "query": request.query,
            "partitionKey": request.partition.key(),
            "resultCount": results.len(),
            "resultIDs": result_ids,
            "resultKinds": result_kinds,
            "requestedScopes": requested_scopes,
            "scopesUsed": scopes_used.clone(),
            "blockedScopes": blocked_scopes.clone(),
            "linkage": linkage,
            "status": search_status,
            "auditID": audit_id,
        }),
    );
    Ok(Json(MemorySearchResponse {
        results,
        scopes_used,
        blocked_scopes,
        audit_id,
    }))
}

pub(super) async fn memory_demote(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Json(input): Json<MemoryDemoteInput>,
) -> Result<Json<Value>, StatusCode> {
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let record = db
        .get_global_memory(&input.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(record) = record else {
        emit_missing_memory_demote_audit(
            &state,
            &tenant_context,
            &input.run_id,
            &input.id,
            "memory not found",
        )
        .await?;
        return Err(StatusCode::NOT_FOUND);
    };
    let changed = db
        .set_global_memory_visibility(&input.id, "private", true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !changed {
        emit_missing_memory_demote_audit(
            &state,
            &tenant_context,
            &input.run_id,
            &input.id,
            "memory not found",
        )
        .await?;
        return Err(StatusCode::NOT_FOUND);
    }
    let partition_key = memory_linkage(&record)
        .get("partition_key")
        .and_then(Value::as_str)
        .unwrap_or("demoted")
        .to_string();
    let demote_detail = format!(
        "kind={} classification={} artifact_refs={} visibility=private tier={} partition_key={} demoted=true{}",
        memory_kind_label(&record.source_type),
        memory_classification_label(record.metadata.as_ref()),
        memory_artifact_refs(record.metadata.as_ref())
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(","),
        tandem_memory::GovernedMemoryTier::Session,
        partition_key,
        memory_linkage_detail(&memory_linkage(&record))
    );
    let audit_id = Uuid::new_v4().to_string();
    append_memory_audit(
        &state,
        &tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_demote".to_string(),
            run_id: input.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: Some(input.id.clone()),
            source_memory_id: None,
            to_tier: None,
            partition_key: partition_key.clone(),
            actor: "system".to_string(),
            status: "ok".to_string(),
            detail: Some(demote_detail),
            created_at_ms: crate::now_ms(),
        },
    )
    .await?;
    publish_tenant_event(
        &state,
        &tenant_context,
        "memory.updated",
        json!({
            "memoryID": input.id,
            "runID": input.run_id,
            "action": "demote",
            "kind": memory_kind_label(&record.source_type),
            "classification": memory_classification_label(record.metadata.as_ref()),
            "artifactRefs": memory_artifact_refs(record.metadata.as_ref()),
            "visibility": "private",
            "tier": tandem_memory::GovernedMemoryTier::Session,
            "partitionKey": partition_key,
            "demoted": true,
            "linkage": memory_linkage(&record),
            "auditID": audit_id,
        }),
    );
    Ok(Json(json!({
        "ok": true,
        "audit_id": audit_id,
    })))
}

#[derive(Debug, Deserialize)]
pub(super) struct ContextResolveUriRequest {
    uri: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContextTreeQuery {
    uri: String,
    max_depth: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContextGenerateLayersRequest {
    node_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ContextDistillRequest {
    session_id: String,
    conversation: Vec<String>,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    workflow_id: Option<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    artifact_refs: Vec<String>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    importance_threshold: Option<f64>,
}

pub(super) async fn context_resolve_uri(
    State(_state): State<AppState>,
    Json(input): Json<ContextResolveUriRequest>,
) -> Result<Json<Value>, StatusCode> {
    let manager = open_memory_manager()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let node = manager
        .resolve_uri(&input.uri)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "node": node })))
}

pub(super) async fn context_tree(
    State(_state): State<AppState>,
    Query(query): Query<ContextTreeQuery>,
) -> Result<Json<Value>, StatusCode> {
    let manager = open_memory_manager()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let max_depth = query.max_depth.unwrap_or(3);
    let tree = manager
        .tree(&query.uri, max_depth)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(json!({ "tree": tree })))
}

pub(super) async fn context_generate_layers(
    State(state): State<AppState>,
    Json(input): Json<ContextGenerateLayersRequest>,
) -> Result<Json<Value>, StatusCode> {
    let runtime_state = state.runtime.wait();
    let providers = runtime_state.providers.clone();

    let manager = open_memory_manager()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    manager
        .generate_layers_for_node(&input.node_id, &providers)
        .await
        .map_err(|e| {
            tracing::warn!("Failed to generate layers: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn context_distill(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Json(input): Json<ContextDistillRequest>,
) -> Result<Json<Value>, StatusCode> {
    let runtime_state = state.runtime.wait();
    let providers = runtime_state.providers.clone();
    let run_id = input
        .run_id
        .clone()
        .unwrap_or_else(|| format!("distill-{}", input.session_id));
    let project_id = input
        .project_id
        .clone()
        .or_else(|| input.workflow_id.clone())
        .unwrap_or_else(|| input.session_id.clone());
    let subject = run_memory_subject(
        input
            .subject
            .as_deref()
            .or(tenant_context.actor_id.as_deref()),
    );
    let partition = tandem_memory::MemoryPartition {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        project_id,
        tier: tandem_memory::GovernedMemoryTier::Session,
    };
    let capability = issue_run_memory_capability(
        &run_id,
        Some(subject.as_str()),
        &partition,
        RunMemoryCapabilityPolicy::CoderWorkflow,
    );
    let writer = GovernedDistillationWriter {
        state: state.clone(),
        tenant_context: tenant_context.clone(),
        partition,
        capability,
        run_id,
        workflow_id: input.workflow_id.clone(),
        artifact_refs: input.artifact_refs.clone(),
        subject,
    };
    let threshold = input.importance_threshold.unwrap_or(0.5).clamp(0.0, 1.0);
    let distiller = tandem_memory::SessionDistiller::with_threshold(Arc::new(providers), threshold);
    let report = distiller
        .distill_with_writer(&input.session_id, &input.conversation, &writer)
        .await
        .map_err(|e| {
            tracing::warn!("Failed to distill session: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let distillation_id = report.distillation_id.clone();
    let session_id = report.session_id.clone();
    let facts_extracted = report.facts_extracted;
    let stored_count = report.stored_count;
    let deduped_count = report.deduped_count;
    let memory_ids = report.memory_ids.clone();
    let candidate_ids = report.candidate_ids.clone();
    let status = report.status.clone();

    Ok(Json(json!({
        "ok": true,
        "distillation_id": distillation_id,
        "session_id": session_id,
        "facts_extracted": facts_extracted,
        "stored_count": stored_count,
        "deduped_count": deduped_count,
        "memory_ids": memory_ids,
        "candidate_ids": candidate_ids,
        "status": status,
        "report": report,
    })))
}

pub(super) async fn workflow_learning_candidates_list(
    State(state): State<AppState>,
    Query(query): Query<WorkflowLearningCandidateListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let status = match query.status.as_deref() {
        Some(value) => {
            Some(workflow_learning_status_from_str(value).ok_or(StatusCode::BAD_REQUEST)?)
        }
        None => None,
    };
    let kind = match query.kind.as_deref() {
        Some(value) => Some(workflow_learning_kind_from_str(value).ok_or(StatusCode::BAD_REQUEST)?),
        None => None,
    };
    let mut candidates = state
        .list_workflow_learning_candidates(query.workflow_id.as_deref(), status, kind)
        .await;
    if let Some(project_id) = query.project_id.as_deref() {
        candidates.retain(|candidate| candidate.project_id == project_id);
    }
    let count = candidates.len();
    Ok(Json(json!({
        "candidates": candidates,
        "count": count,
    })))
}

pub(super) async fn workflow_learning_candidate_review(
    State(state): State<AppState>,
    Path(candidate_id): Path<String>,
    Json(input): Json<WorkflowLearningCandidateReviewRequest>,
) -> Result<Json<Value>, StatusCode> {
    let Some(candidate) = state.get_workflow_learning_candidate(&candidate_id).await else {
        return Err(StatusCode::NOT_FOUND);
    };
    let action = input
        .action
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("approve")
        .to_ascii_lowercase();
    let next_status = match action.as_str() {
        "approve" | "approved" => WorkflowLearningCandidateStatus::Approved,
        "reject" | "rejected" => WorkflowLearningCandidateStatus::Rejected,
        "applied" => WorkflowLearningCandidateStatus::Applied,
        "supersede" | "superseded" => WorkflowLearningCandidateStatus::Superseded,
        "regress" | "regressed" => WorkflowLearningCandidateStatus::Regressed,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let baseline = if matches!(
        next_status,
        WorkflowLearningCandidateStatus::Approved | WorkflowLearningCandidateStatus::Applied
    ) {
        Some(
            state
                .workflow_learning_metrics_for_workflow(&candidate.workflow_id)
                .await,
        )
    } else {
        None
    };
    let reviewed_at_ms = crate::now_ms();
    let updated = state
        .update_workflow_learning_candidate(&candidate_id, |candidate| {
            candidate.status = next_status;
            if candidate.baseline_before.is_none() {
                candidate.baseline_before = baseline.clone();
            }
            if let Some(note) = input
                .note
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                candidate.evidence_refs.push(json!({
                    "review_note": note,
                    "reviewer_id": input.reviewer_id,
                    "reviewed_at_ms": reviewed_at_ms,
                    "action": action,
                }));
            }
        })
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({
        "ok": true,
        "candidate": updated,
    })))
}
