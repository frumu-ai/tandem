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

pub(super) async fn open_global_memory_db_for_state(state: &AppState) -> Option<MemoryDatabase> {
    if let Some(parent) = state.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    MemoryDatabase::new(&state.memory_db_path).await.ok()
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

pub(super) async fn open_memory_manager_for_state(
    state: &AppState,
) -> Option<tandem_memory::MemoryManager> {
    if let Some(parent) = state.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tandem_memory::MemoryManager::new(&state.memory_db_path)
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
    let Some(db) = open_global_memory_db_for_state(&state).await else {
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

pub(super) async fn memory_import(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<MemoryImportInput>,
) -> Result<Json<MemoryImportResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    let source_kind = input.source.kind.trim().to_ascii_lowercase();
    if source_kind != "path" {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source.kind must be `path`",
        ));
    }

    let path = input.source.path.trim().to_string();
    if path.is_empty() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source.path is required for path imports",
        ));
    }

    validate_memory_import_path(&path)?;

    let project_id = normalize_optional_memory_import_id(input.project_id);
    let session_id = normalize_optional_memory_import_id(input.session_id);
    let source_binding_id = normalize_optional_memory_import_id(input.source_binding_id);
    match input.tier {
        MemoryTier::Project if project_id.is_none() => {
            return Err(skill_error(
                StatusCode::BAD_REQUEST,
                "tier=project requires project_id",
            ));
        }
        MemoryTier::Session if session_id.is_none() => {
            return Err(skill_error(
                StatusCode::BAD_REQUEST,
                "tier=session requires session_id",
            ));
        }
        _ => {}
    }
    let source_binding = resolve_memory_import_source_binding(
        &state,
        &tenant_context,
        &request_principal,
        verified_tenant_context.as_deref(),
        source_binding_id.as_deref(),
    )
    .await?;
    let source_binding_for_job = source_binding.clone();
    let job_started_at_ms = crate::util::time::now_ms();
    let ingestion_job_id = source_binding_for_job.as_ref().map(|binding| {
        format!(
            "manual-import-{}-{}",
            job_started_at_ms,
            uuid::Uuid::new_v4()
        )
    });

    if let (Some(job_id), Some(binding)) = (&ingestion_job_id, source_binding_for_job.as_ref()) {
        if let Err(err) = record_enterprise_ingestion_job(
            &state,
            IngestionJob {
                job_id: job_id.clone(),
                tenant_context: tenant_context.clone(),
                connector_id: binding.connector_id.clone(),
                binding_id: binding.binding_id.clone(),
                state: IngestionJobState::Running,
                source_object_ids: Vec::new(),
                started_at_ms: Some(job_started_at_ms),
                finished_at_ms: None,
                quarantine_id: None,
            },
        )
        .await
        {
            tracing::warn!(
                error = %err,
                "failed to record enterprise ingestion job start"
            );
        }
    }

    publish_tenant_event(
        &state,
        &tenant_context,
        "memory.import.started",
        json!({
            "source": {"kind": "path", "path": path},
            "format": input.format,
            "tier": input.tier,
            "project_id": project_id.clone(),
            "session_id": session_id.clone(),
            "source_binding_id": source_binding_id.clone(),
            "sync_deletes": input.sync_deletes,
        }),
    );

    let Some(manager) = open_memory_manager_for_state(&state).await else {
        if let (Some(job_id), Some(binding)) = (&ingestion_job_id, source_binding_for_job.as_ref())
        {
            if let Err(err) = record_enterprise_ingestion_job(
                &state,
                IngestionJob {
                    job_id: job_id.clone(),
                    tenant_context: tenant_context.clone(),
                    connector_id: binding.connector_id.clone(),
                    binding_id: binding.binding_id.clone(),
                    state: IngestionJobState::Failed,
                    source_object_ids: Vec::new(),
                    started_at_ms: Some(job_started_at_ms),
                    finished_at_ms: Some(crate::util::time::now_ms()),
                    quarantine_id: None,
                },
            )
            .await
            {
                tracing::warn!(
                    error = %err,
                    "failed to record enterprise ingestion job failure"
                );
            }
        }
        publish_tenant_event(
            &state,
            &tenant_context,
            "memory.import.failed",
            json!({
                "source": {"kind": "path", "path": path},
                "format": input.format,
                "tier": input.tier,
                "source_binding_id": source_binding_id.clone(),
                "error": "failed to open memory manager",
            }),
        );
        return Err(skill_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to open memory manager",
        ));
    };

    let request = TandemMemoryImportRequest {
        root_path: path.clone(),
        format: input.format,
        tier: input.tier,
        session_id: session_id.clone(),
        project_id: project_id.clone(),
        tenant_scope: MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        },
        source_binding,
        sync_deletes: input.sync_deletes,
        import_namespace: None,
    };

    let stats = match import_files(&manager, &request, None::<fn(&MemoryImportProgress)>).await {
        Ok(stats) => stats,
        Err(err) => {
            if let (Some(job_id), Some(binding)) =
                (&ingestion_job_id, source_binding_for_job.as_ref())
            {
                if let Err(record_err) = record_enterprise_ingestion_job(
                    &state,
                    IngestionJob {
                        job_id: job_id.clone(),
                        tenant_context: tenant_context.clone(),
                        connector_id: binding.connector_id.clone(),
                        binding_id: binding.binding_id.clone(),
                        state: IngestionJobState::Failed,
                        source_object_ids: Vec::new(),
                        started_at_ms: Some(job_started_at_ms),
                        finished_at_ms: Some(crate::util::time::now_ms()),
                        quarantine_id: None,
                    },
                )
                .await
                {
                    tracing::warn!(
                        error = %record_err,
                        "failed to record enterprise ingestion job failure"
                    );
                }
            }
            publish_tenant_event(
                &state,
                &tenant_context,
                "memory.import.failed",
                json!({
                    "source": {"kind": "path", "path": path},
                    "format": input.format,
                    "tier": input.tier,
                    "project_id": project_id.clone(),
                    "session_id": session_id.clone(),
                    "source_binding_id": source_binding_id.clone(),
                    "sync_deletes": input.sync_deletes,
                    "error": err.to_string(),
                }),
            );
            return Err(skill_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("memory import failed: {err}"),
            ));
        }
    };

    if let (Some(job_id), Some(binding)) = (&ingestion_job_id, source_binding_for_job.as_ref()) {
        let source_objects = source_objects_seen_since(
            &manager,
            &request.tenant_scope,
            &binding.binding_id,
            job_started_at_ms,
        )
        .await
        .unwrap_or_default();
        let source_object_ids = source_objects
            .iter()
            .map(|record| record.source_object_id.clone())
            .collect::<Vec<_>>();
        let quarantine_id = if binding.require_review {
            let quarantine_id =
                format!("quarantine-{}-{}", job_started_at_ms, uuid::Uuid::new_v4());
            if let Err(err) = quarantine_source_bound_import(
                &manager,
                &request.tenant_scope,
                &binding.binding_id,
                &source_objects,
                job_started_at_ms,
            )
            .await
            {
                tracing::warn!(
                    error = %err,
                    "failed to quarantine enterprise source-bound import output"
                );
            }
            if let Err(err) = record_enterprise_ingestion_quarantine(
                &state,
                IngestionQuarantine {
                    quarantine_id: quarantine_id.clone(),
                    tenant_context: tenant_context.clone(),
                    connector_id: binding.connector_id.clone(),
                    binding_id: binding.binding_id.clone(),
                    source_object_ids: source_object_ids.clone(),
                    reason: "source binding requires ingestion review".to_string(),
                    created_at_ms: crate::util::time::now_ms(),
                    reviewed_by: None,
                    reviewed_at_ms: None,
                    disposition: None,
                },
            )
            .await
            {
                tracing::warn!(
                    error = %err,
                    "failed to record enterprise ingestion quarantine"
                );
            }
            Some(quarantine_id)
        } else {
            None
        };
        let job_state = if binding.require_review {
            IngestionJobState::Quarantined
        } else if stats.errors > 0 {
            IngestionJobState::Failed
        } else {
            IngestionJobState::Completed
        };
        if let Err(err) = record_enterprise_ingestion_job(
            &state,
            IngestionJob {
                job_id: job_id.clone(),
                tenant_context: tenant_context.clone(),
                connector_id: binding.connector_id.clone(),
                binding_id: binding.binding_id.clone(),
                state: job_state,
                source_object_ids,
                started_at_ms: Some(job_started_at_ms),
                finished_at_ms: Some(crate::util::time::now_ms()),
                quarantine_id,
            },
        )
        .await
        {
            tracing::warn!(
                error = %err,
                "failed to record enterprise ingestion job completion"
            );
        }
    }

    publish_tenant_event(
        &state,
        &tenant_context,
        "memory.import.succeeded",
        json!({
            "source": {"kind": "path", "path": path},
            "format": input.format,
            "tier": input.tier,
            "project_id": project_id.clone(),
            "session_id": session_id.clone(),
            "source_binding_id": source_binding_id.clone(),
            "sync_deletes": input.sync_deletes,
            "stats": {
                "discovered_files": stats.discovered_files,
                "files_processed": stats.files_processed,
                "indexed_files": stats.indexed_files,
                "skipped_files": stats.skipped_files,
                "deleted_files": stats.deleted_files,
                "chunks_created": stats.chunks_created,
                "errors": stats.errors,
            },
        }),
    );

    Ok(Json(memory_import_response(
        path,
        input.format,
        input.tier,
        project_id,
        session_id,
        source_binding_id,
        input.sync_deletes,
        stats,
    )))
}

async fn resolve_memory_import_source_binding(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    source_binding_id: Option<&str>,
) -> Result<Option<MemoryImportSourceBinding>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(source_binding_id) = source_binding_id else {
        if memory_import_requires_source_binding(
            tenant_context,
            request_principal,
            verified_tenant_context,
        ) {
            return Err(skill_error(
                StatusCode::BAD_REQUEST,
                "hosted/enterprise memory imports require source_binding_id",
            ));
        }
        return Ok(None);
    };
    if source_binding_id == DEFAULT_LOCAL_MANUAL_SOURCE_BINDING_ID
        && !memory_import_requires_source_binding(
            tenant_context,
            request_principal,
            verified_tenant_context,
        )
    {
        return Ok(Some(default_local_manual_source_binding(tenant_context)));
    }
    let registry = state.enterprise_source_bindings.read().await;
    let Some(binding) = registry.values().find(|binding| {
        binding.binding_id == source_binding_id && binding.tenant_matches(tenant_context)
    }) else {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source_binding_id does not reference an enabled binding for this tenant",
        ));
    };
    if !binding.state.allows_ingestion() || !binding.ingestion_policy.allow_indexing {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source binding does not allow memory import indexing",
        ));
    }
    let registry = state.enterprise_connectors.read().await;
    let Some(connector) = registry.values().find(|connector| {
        connector.connector_id == binding.connector_id && connector.tenant_matches(tenant_context)
    }) else {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source binding connector is not registered for this tenant",
        ));
    };
    if !connector.state.allows_ingestion() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            format!(
                "source binding connector does not allow memory import indexing: {}",
                connector_lifecycle_state_label(connector.state)
            ),
        ));
    }
    Ok(Some(MemoryImportSourceBinding {
        binding_id: binding.binding_id.clone(),
        connector_id: binding.connector_id.clone(),
        resource_ref: serde_json::to_value(&binding.resource_ref).map_err(|_| {
            skill_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to serialize source binding resource scope",
            )
        })?,
        data_class: serde_json::to_value(binding.data_class)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| format!("{:?}", binding.data_class)),
        require_review: binding.ingestion_policy.require_review,
    }))
}

const DEFAULT_LOCAL_MANUAL_SOURCE_BINDING_ID: &str = "local_manual_upload";

fn default_local_manual_source_binding(
    tenant_context: &TenantContext,
) -> MemoryImportSourceBinding {
    MemoryImportSourceBinding {
        binding_id: DEFAULT_LOCAL_MANUAL_SOURCE_BINDING_ID.to_string(),
        connector_id: "manual_upload".to_string(),
        resource_ref: json!({
            "organization_id": tenant_context.org_id.clone(),
            "workspace_id": tenant_context.workspace_id.clone(),
            "resource_kind": "document_collection",
            "resource_id": "local-manual-uploads",
        }),
        data_class: "internal".to_string(),
        require_review: false,
    }
}

async fn record_enterprise_ingestion_job(
    state: &AppState,
    job: IngestionJob,
) -> Result<(), std::io::Error> {
    let mut registry = state.enterprise_ingestion_jobs.write().await;
    let key = enterprise_ingestion_job_key(&job);
    registry.insert(key, job);
    persist_enterprise_ingestion_jobs(&state.enterprise_ingestion_jobs_path, &registry).await
}

async fn record_enterprise_ingestion_quarantine(
    state: &AppState,
    quarantine: IngestionQuarantine,
) -> Result<(), std::io::Error> {
    let mut registry = state.enterprise_ingestion_quarantines.write().await;
    let key = enterprise_ingestion_quarantine_key(&quarantine);
    registry.insert(key, quarantine);
    persist_enterprise_ingestion_quarantines(
        &state.enterprise_ingestion_quarantines_path,
        &registry,
    )
    .await
}

fn enterprise_ingestion_job_key(job: &IngestionJob) -> String {
    let deployment = job
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        job.tenant_context.org_id, job.tenant_context.workspace_id, deployment, job.job_id
    )
}

fn enterprise_ingestion_quarantine_key(quarantine: &IngestionQuarantine) -> String {
    let deployment = quarantine
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        quarantine.tenant_context.org_id,
        quarantine.tenant_context.workspace_id,
        deployment,
        quarantine.quarantine_id
    )
}

async fn persist_enterprise_ingestion_jobs(
    path: &std::path::Path,
    registry: &std::collections::HashMap<String, IngestionJob>,
) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let payload = serde_json::to_vec_pretty(registry).map_err(std::io::Error::other)?;
    tokio::fs::write(path, payload).await
}

async fn persist_enterprise_ingestion_quarantines(
    path: &std::path::Path,
    registry: &std::collections::HashMap<String, IngestionQuarantine>,
) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let payload = serde_json::to_vec_pretty(registry).map_err(std::io::Error::other)?;
    tokio::fs::write(path, payload).await
}

async fn source_objects_seen_since(
    manager: &tandem_memory::MemoryManager,
    tenant_scope: &MemoryTenantScope,
    binding_id: &str,
    started_at_ms: u64,
) -> Result<Vec<SourceObjectLifecycleRecord>, tandem_memory::types::MemoryError> {
    let mut records: Vec<_> = manager
        .db()
        .list_source_object_lifecycle_for_binding_for_tenant(tenant_scope, binding_id)
        .await?
        .into_iter()
        .filter(|record| record.last_seen_at_ms >= started_at_ms)
        .collect();
    records.sort_by(|left, right| left.source_object_id.cmp(&right.source_object_id));
    records.dedup_by(|left, right| left.source_object_id == right.source_object_id);
    Ok(records)
}

async fn quarantine_source_bound_import(
    manager: &tandem_memory::MemoryManager,
    tenant_scope: &MemoryTenantScope,
    binding_id: &str,
    source_objects: &[SourceObjectLifecycleRecord],
    changed_at_ms: u64,
) -> Result<(), tandem_memory::types::MemoryError> {
    for record in source_objects {
        manager
            .db()
            .delete_file_chunks_by_path_for_tenant(
                record.tier,
                record.session_id.as_deref(),
                record.project_id.as_deref(),
                &record.indexed_path,
                tenant_scope,
            )
            .await?;
        manager
            .db()
            .delete_import_index_entry_for_tenant(
                record.tier,
                record.session_id.as_deref(),
                record.project_id.as_deref(),
                &record.indexed_path,
                tenant_scope,
            )
            .await?;
        manager
            .db()
            .mark_source_object_lifecycle_state_for_tenant(
                tenant_scope,
                binding_id,
                &record.source_object_id,
                SourceObjectLifecycleState::Quarantined,
                changed_at_ms,
            )
            .await?;
    }
    Ok(())
}

fn connector_lifecycle_state_label(state: ConnectorLifecycleState) -> &'static str {
    match state {
        ConnectorLifecycleState::Active => "active",
        ConnectorLifecycleState::Paused => "paused",
        ConnectorLifecycleState::Revoked => "revoked",
        ConnectorLifecycleState::Quarantined => "quarantined",
    }
}

fn memory_import_requires_source_binding(
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> bool {
    verified_tenant_context.is_some()
        || tenant_context.deployment_id.is_some()
        || request_principal.source == "tandem-web"
}

fn memory_import_response(
    path: String,
    format: MemoryImportFormat,
    tier: MemoryTier,
    project_id: Option<String>,
    session_id: Option<String>,
    source_binding_id: Option<String>,
    sync_deletes: bool,
    stats: MemoryImportStats,
) -> MemoryImportResponse {
    MemoryImportResponse {
        ok: true,
        source: MemoryImportPathSourceResponse { kind: "path", path },
        format,
        tier,
        project_id,
        session_id,
        source_binding_id,
        sync_deletes,
        discovered_files: stats.discovered_files,
        files_processed: stats.files_processed,
        indexed_files: stats.indexed_files,
        skipped_files: stats.skipped_files,
        deleted_files: stats.deleted_files,
        chunks_created: stats.chunks_created,
        errors: stats.errors,
    }
}

fn normalize_optional_memory_import_id(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .filter(|trimmed| !trimmed.is_empty())
}

fn validate_memory_import_path(path: &str) -> Result<(), (StatusCode, Json<ErrorEnvelope>)> {
    let metadata = std::fs::metadata(path).map_err(|err| {
        skill_error(
            StatusCode::BAD_REQUEST,
            format!("source.path must exist and be readable: {err}"),
        )
    })?;

    let readable = if metadata.is_dir() {
        std::fs::read_dir(path).map(|_| ())
    } else {
        std::fs::File::open(path).map(|_| ())
    };
    readable.map_err(|err| {
        skill_error(
            StatusCode::BAD_REQUEST,
            format!("source.path must be readable: {err}"),
        )
    })
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
    let db = open_global_memory_db_for_state(state)
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
