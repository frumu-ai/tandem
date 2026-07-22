#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionMeta {
    pub parent_id: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub shared: bool,
    pub share_id: Option<String>,
    pub summary: Option<String>,
    #[serde(default)]
    pub snapshots: Vec<Vec<Message>>,
    pub pre_revert: Option<Vec<Message>>,
    #[serde(default)]
    pub todos: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionToolRef {
    #[serde(rename = "callID")]
    pub call_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionRequest {
    pub id: String,
    #[serde(default = "TenantContext::local_implicit", rename = "tenantContext")]
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "requestedBy")]
    pub requested_by: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty", rename = "actionDigest")]
    pub action_digest: String,
    #[serde(default, rename = "expiresAtMs")]
    pub expires_at_ms: u64,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(default)]
    pub questions: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<QuestionToolRef>,
}

pub struct Storage {
    base: PathBuf,
    repository: session_repository::SessionRepository,
    question_write_lock: tokio::sync::Mutex<()>,
}

#[derive(Debug, Clone)]
pub enum SessionListScope {
    Global,
    Workspace { workspace_root: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionRepairStats {
    pub sessions_repaired: u64,
    pub messages_recovered: u64,
    pub parts_recovered: u64,
    pub conflicts_merged: u64,
}

const LEGACY_IMPORT_MARKER_FILE: &str = "legacy_import_marker.json";
const LEGACY_IMPORT_MARKER_VERSION: u32 = 1;
const MAX_SESSION_SNAPSHOTS: usize = 5;
const SESSIONS_SCHEMA_VERSION: u32 = 1;
const QUESTION_REQUEST_TTL_MS: u64 = 15 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionsFile {
    schema_version: u32,
    sessions: HashMap<String, Session>,
}

fn load_sessions_file(raw: &str) -> anyhow::Result<(HashMap<String, Session>, bool)> {
    let value: Value = serde_json::from_str(raw).context("failed to parse sessions.json")?;
    let Some(version_value) = value.get("schema_version") else {
        let sessions = serde_json::from_value::<HashMap<String, Session>>(value)
            .context("failed to parse legacy sessions.json v0 map")?;
        return Ok((upgrade_sessions_file(0, sessions)?, true));
    };

    let schema_version = version_value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .context("sessions.json schema_version must be an unsigned integer")?;
    if schema_version > SESSIONS_SCHEMA_VERSION {
        anyhow::bail!(
            "sessions.json schema_version {} is newer than supported version {}",
            schema_version,
            SESSIONS_SCHEMA_VERSION
        );
    }

    let file = serde_json::from_value::<SessionsFile>(value)
        .context("failed to parse versioned sessions.json")?;
    let upgraded = file.schema_version < SESSIONS_SCHEMA_VERSION;
    Ok((
        upgrade_sessions_file(file.schema_version, file.sessions)?,
        upgraded,
    ))
}

fn upgrade_sessions_file(
    from_version: u32,
    sessions: HashMap<String, Session>,
) -> anyhow::Result<HashMap<String, Session>> {
    let mut current = from_version;
    while current < SESSIONS_SCHEMA_VERSION {
        match current {
            0 => {
                current = 1;
            }
            other => anyhow::bail!("unsupported sessions.json schema version {}", other),
        }
    }
    Ok(sessions)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LegacyTreeCounts {
    pub session_files: u64,
    pub message_files: u64,
    pub part_files: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LegacyImportedCounts {
    pub sessions: u64,
    pub messages: u64,
    pub parts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyImportMarker {
    pub version: u32,
    pub created_at_ms: u64,
    pub last_checked_at_ms: u64,
    pub legacy_counts: LegacyTreeCounts,
    pub imported_counts: LegacyImportedCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyRepairRunReport {
    pub status: String,
    pub marker_updated: bool,
    pub sessions_merged: u64,
    pub messages_recovered: u64,
    pub parts_recovered: u64,
    pub legacy_counts: LegacyTreeCounts,
    pub imported_counts: LegacyImportedCounts,
}

fn snapshot_session_messages(
    session_id: &str,
    session: &Session,
    metadata: &mut HashMap<String, SessionMeta>,
) {
    let meta = metadata
        .entry(session_id.to_string())
        .or_insert_with(SessionMeta::default);
    meta.snapshots.push(session.messages.clone());
    trim_session_snapshots(&mut meta.snapshots);
}

fn trim_session_snapshots(snapshots: &mut Vec<Vec<Message>>) {
    if snapshots.len() > MAX_SESSION_SNAPSHOTS {
        let keep_from = snapshots.len() - MAX_SESSION_SNAPSHOTS;
        snapshots.drain(0..keep_from);
    }
}

fn compact_session_snapshots(snapshots: &mut Vec<Vec<Message>>) -> usize {
    if snapshots.is_empty() {
        return 0;
    }

    let original_len = snapshots.len();
    let mut compacted = Vec::with_capacity(original_len);
    let mut previous_encoded: Option<Vec<u8>> = None;

    for snapshot in snapshots.drain(..) {
        let encoded = serde_json::to_vec(&snapshot).unwrap_or_default();
        if previous_encoded.as_ref() == Some(&encoded) {
            continue;
        }
        previous_encoded = Some(encoded);
        compacted.push(snapshot);
    }

    trim_session_snapshots(&mut compacted);
    let removed = original_len.saturating_sub(compacted.len());
    *snapshots = compacted;
    removed
}

fn session_matches_normalized_workspace(session: &Session, normalized_workspace: &str) -> bool {
    session
        .workspace_root
        .as_ref()
        .and_then(|p| normalize_workspace_path(p))
        .map(|p| p == normalized_workspace)
        .unwrap_or(false)
        || normalize_workspace_path(&session.directory)
            .map(|p| p == normalized_workspace)
            .unwrap_or(false)
}

fn session_summary_without_messages(session: &Session) -> Session {
    Session {
        id: session.id.clone(),
        slug: session.slug.clone(),
        version: session.version.clone(),
        project_id: session.project_id.clone(),
        title: session.title.clone(),
        directory: session.directory.clone(),
        workspace_root: session.workspace_root.clone(),
        pinned_workspace_id: session.pinned_workspace_id.clone(),
        origin_workspace_root: session.origin_workspace_root.clone(),
        attached_from_workspace: session.attached_from_workspace.clone(),
        attached_to_workspace: session.attached_to_workspace.clone(),
        attach_timestamp_ms: session.attach_timestamp_ms,
        attach_reason: session.attach_reason.clone(),
        tenant_context: session.tenant_context.clone(),
        verified_tenant_context: session.verified_tenant_context.clone(),
        time: session.time.clone(),
        model: session.model.clone(),
        provider: session.provider.clone(),
        sampling: session.sampling,
        source_kind: session.source_kind.clone(),
        source_metadata: session.source_metadata.clone(),
        environment: session.environment.clone(),
        messages: Vec::new(),
    }
}

fn session_meta_is_empty(meta: &SessionMeta) -> bool {
    meta.parent_id.is_none()
        && !meta.archived
        && !meta.shared
        && meta.share_id.is_none()
        && meta.summary.is_none()
        && meta.snapshots.is_empty()
        && meta.pre_revert.is_none()
        && meta.todos.is_empty()
}

#[derive(Debug, Default)]
struct SessionMetaCompactionStats {
    metadata_pruned: u64,
    snapshots_removed: u64,
}

fn automation_v2_source_metadata_from_title(title: &str) -> Option<(String, serde_json::Value)> {
    let title = title.trim_start();
    let rest = title.strip_prefix("Automation ")?;
    let (automation_id, node_id) = rest.split_once(" / ")?;
    let node_id = node_id.trim().trim_end_matches(" (Reused)");
    Some((
        "automation_v2".to_string(),
        serde_json::json!({
            "automation_id": automation_id.trim(),
            "node_id": node_id,
        }),
    ))
}

fn compact_session_metadata(
    sessions: &HashMap<String, Session>,
    metadata: &mut HashMap<String, SessionMeta>,
) -> SessionMetaCompactionStats {
    let mut stats = SessionMetaCompactionStats::default();

    metadata.retain(|session_id, meta| {
        if !sessions.contains_key(session_id) {
            stats.metadata_pruned += 1;
            return false;
        }

        let removed = compact_session_snapshots(&mut meta.snapshots) as u64;
        stats.snapshots_removed += removed;

        if session_meta_is_empty(meta) {
            stats.metadata_pruned += 1;
            return false;
        }

        true
    });

    stats
}

impl Storage {
    pub async fn new(base: impl AsRef<Path>) -> anyhow::Result<Self> {
        let base = base.as_ref().to_path_buf();
        fs::create_dir_all(&base).await?;
        let repository = session_repository::SessionRepository::open(&base)?;
        if !repository.is_imported()? {
            let base_for_import = base.clone();
            let legacy_state = task::spawn_blocking(move || collect_legacy_import_state(&base_for_import))
                .await
                .context("legacy session import task failed")??;
            repository.import_legacy(legacy_state)?;
        }
        Ok(Self {
            base,
            repository,
            question_write_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub async fn list_sessions(&self) -> Vec<Session> {
        self.list_sessions_scoped(SessionListScope::Global).await
    }

    pub async fn list_session_summaries(&self) -> Vec<Session> {
        self.list_session_summaries_scoped(SessionListScope::Global).await
    }

    pub async fn list_sessions_scoped(&self, scope: SessionListScope) -> Vec<Session> {
        let is_workspace_scope = matches!(&scope, SessionListScope::Workspace { .. });
        let workspace_root = match scope {
            SessionListScope::Global => None,
            SessionListScope::Workspace { workspace_root } => normalize_workspace_path(&workspace_root),
        };
        if is_workspace_scope && workspace_root.is_none() {
            return Vec::new();
        }
        self.run_blocking(move |repository| repository.list_sessions(workspace_root.as_deref()))
            .await
            .unwrap_or_else(|error| {
                tracing::error!(%error, "failed to list sessions from transactional store");
                Vec::new()
            })
    }

    pub async fn list_session_summaries_scoped(&self, scope: SessionListScope) -> Vec<Session> {
        let is_workspace_scope = matches!(&scope, SessionListScope::Workspace { .. });
        let workspace_root = match scope {
            SessionListScope::Global => None,
            SessionListScope::Workspace { workspace_root } => normalize_workspace_path(&workspace_root),
        };
        if is_workspace_scope && workspace_root.is_none() {
            return Vec::new();
        }
        self.run_blocking(move |repository| repository.list_summaries(workspace_root.as_deref()))
            .await
            .unwrap_or_else(|error| {
                tracing::error!(%error, "failed to list session summaries from transactional store");
                Vec::new()
            })
    }

    pub async fn get_session(&self, id: &str) -> Option<Session> {
        let id = id.to_string();
        let query_id = id.clone();
        self.run_blocking(move |repository| repository.get_session(&query_id))
            .await
            .unwrap_or_else(|error| {
                tracing::error!(%error, session_id = %id, "failed to read session from transactional store");
                None
            })
    }

    pub async fn save_session(&self, mut session: Session) -> anyhow::Result<()> {
        if session.workspace_root.is_none() {
            session.workspace_root = normalize_workspace_path(&session.directory);
        }
        if session.source_kind.is_none() {
            if let Some((source_kind, source_metadata)) = automation_v2_source_metadata_from_title(&session.title) {
                session.source_kind = Some(source_kind);
                session.source_metadata = Some(source_metadata);
            }
        }
        if title_needs_repair(&session.title) {
            let first_user_text = session.messages.iter().find_map(|message| {
                if !matches!(message.role, MessageRole::User) {
                    return None;
                }
                message.parts.iter().find_map(|part| match part {
                    MessagePart::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
                    _ => None,
                })
            });
            if let Some(title) = first_user_text.and_then(|text| derive_session_title_from_prompt(text, 60)) {
                session.title = title;
                session.time.updated = Utc::now();
            }
        }
        self.run_blocking(move |repository| repository.save_session(&session)).await
    }

    pub async fn repair_sessions_from_file_store(&self) -> anyhow::Result<SessionRepairStats> {
        let base = self.base.clone();
        self.run_blocking(move |repository| {
            repository.repair_sessions(move |session| {
                let imported = load_legacy_session_messages(&base, &session.id);
                if imported.is_empty() {
                    return None;
                }
                let (merged, merge_stats, changed) = merge_session_messages(&session.messages, &imported);
                if !changed {
                    return None;
                }
                let mut repaired = session.clone();
                repaired.messages = merged;
                repaired.time.updated = most_recent_message_time(&repaired.messages)
                    .unwrap_or(repaired.time.updated);
                Some((repaired, SessionRepairStats {
                    sessions_repaired: 1,
                    messages_recovered: merge_stats.messages_recovered,
                    parts_recovered: merge_stats.parts_recovered,
                    conflicts_merged: merge_stats.conflicts_merged,
                }))
            })
        }).await
    }

    pub async fn run_legacy_storage_repair_scan(&self, force: bool) -> anyhow::Result<LegacyRepairRunReport> {
        if !force {
            return Ok(LegacyRepairRunReport {
                status: "skipped".to_string(),
                marker_updated: false,
                sessions_merged: 0,
                messages_recovered: 0,
                parts_recovered: 0,
                legacy_counts: LegacyTreeCounts::default(),
                imported_counts: LegacyImportedCounts::default(),
            });
        }
        let base = self.base.clone();
        self.run_blocking(move |repository| {
            let scan = scan_legacy_sessions(&base)?;
            let mut sessions = repository
                .list_sessions(None)?
                .into_iter()
                .map(|session| (session.id.clone(), session))
                .collect::<HashMap<_, _>>();
            let merge = merge_legacy_sessions_with_stats(&mut sessions, scan.sessions);
            if merge.changed {
                for session in sessions.values() {
                    repository.save_session(session)?;
                }
            }
            Ok(LegacyRepairRunReport {
                status: if merge.changed { "updated" } else { "no_changes" }.to_string(),
                marker_updated: false,
                sessions_merged: merge.sessions_merged,
                messages_recovered: merge.messages_recovered,
                parts_recovered: merge.parts_recovered,
                legacy_counts: scan.legacy_counts,
                imported_counts: scan.imported_counts,
            })
        }).await
    }

    pub async fn delete_session(&self, id: &str) -> anyhow::Result<bool> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.delete_session(&id)).await
    }

    pub async fn append_message(&self, session_id: &str, message: Message) -> anyhow::Result<()> {
        let session_id = session_id.to_string();
        self.run_blocking(move |repository| repository.append_message(&session_id, &message)).await
    }

    pub async fn append_message_part(
        &self,
        session_id: &str,
        message_id: &str,
        part: MessagePart,
    ) -> anyhow::Result<()> {
        let session_id = session_id.to_string();
        let message_id = message_id.to_string();
        self.run_blocking(move |repository| {
            repository.append_message_part(&session_id, &message_id, &part)
        }).await
    }

    pub async fn fork_session(&self, id: &str) -> anyhow::Result<Option<Session>> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.fork_session(&id)).await
    }

    pub async fn revert_session(&self, id: &str) -> anyhow::Result<bool> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.revert_session(&id)).await
    }

    pub async fn unrevert_session(&self, id: &str) -> anyhow::Result<bool> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.unrevert_session(&id)).await
    }

    pub async fn set_shared(&self, id: &str, shared: bool) -> anyhow::Result<Option<String>> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.set_shared(&id, shared)).await
    }

    pub async fn set_archived(&self, id: &str, archived: bool) -> anyhow::Result<bool> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.set_archived(&id, archived)).await
    }

    pub async fn set_summary(&self, id: &str, summary: String) -> anyhow::Result<bool> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.set_summary(&id, summary)).await
    }

    pub async fn children(&self, parent_id: &str) -> Vec<Session> {
        let parent_id = parent_id.to_string();
        self.run_blocking(move |repository| repository.children(&parent_id)).await.unwrap_or_default()
    }

    pub async fn session_status(&self, id: &str) -> Option<Value> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.session_status(&id)).await.ok().flatten()
    }

    pub async fn session_diff(&self, id: &str) -> Option<Value> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.session_diff(&id)).await.ok().flatten()
    }

    pub async fn set_todos(&self, id: &str, todos: Vec<Value>) -> anyhow::Result<()> {
        let id = id.to_string();
        let todos = normalize_todo_items(todos);
        self.run_blocking(move |repository| repository.set_todos(&id, todos)).await
    }

    pub async fn get_todos(&self, id: &str) -> Vec<Value> {
        let id = id.to_string();
        self.run_blocking(move |repository| repository.get_todos(&id)).await
            .map(normalize_todo_items)
            .unwrap_or_default()
    }

    pub async fn add_question_request(
        &self,
        session_id: &str,
        message_id: &str,
        questions: Vec<Value>,
    ) -> anyhow::Result<QuestionRequest> {
        if questions.is_empty() {
            anyhow::bail!("cannot add empty question request for session {}", session_id);
        }
        let tenant_context = self
            .get_session(session_id)
            .await
            .map(|session| session.tenant_context)
            .unwrap_or_else(TenantContext::local_implicit);
        let requested_at_ms = now_ms_u64();
        let tool = QuestionToolRef {
            call_id: format!("call-{}", Uuid::new_v4()),
            message_id: message_id.to_string(),
        };
        let digest_payload = json!({
            "tenant": &tenant_context,
            "sessionID": session_id,
            "questions": &questions,
            "tool": &tool,
        });
        let request = QuestionRequest {
            id: format!("q-{}", Uuid::new_v4()),
            requested_by: tenant_context.actor_id.clone(),
            tenant_context,
            action_digest: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&digest_payload).unwrap_or_default())
            ),
            expires_at_ms: requested_at_ms.saturating_add(QUESTION_REQUEST_TTL_MS),
            session_id: session_id.to_string(),
            questions,
            tool: Some(tool),
        };
        let request_for_store = request.clone();
        let _write_guard = self.question_write_lock.lock().await;
        self.run_blocking(move |repository| repository.add_question(&request_for_store)).await?;
        Ok(request)
    }

    pub async fn list_question_requests(&self) -> Vec<QuestionRequest> {
        self.run_blocking(|repository| repository.list_questions()).await.unwrap_or_default()
    }

    pub async fn list_question_requests_for_tenant(
        &self,
        tenant_context: &TenantContext,
    ) -> Vec<QuestionRequest> {
        self.list_question_requests()
            .await
            .into_iter()
            .filter(|request| {
                request.tenant_context.org_id == tenant_context.org_id
                    && request.tenant_context.workspace_id == tenant_context.workspace_id
                    && request.tenant_context.deployment_id == tenant_context.deployment_id
            })
            .collect()
    }

    pub async fn get_question_request_for_tenant(
        &self,
        request_id: &str,
        tenant_context: &TenantContext,
        expected_session_id: Option<&str>,
    ) -> anyhow::Result<Option<QuestionRequest>> {
        let Some(request) = self
            .list_question_requests_for_tenant(tenant_context)
            .await
            .into_iter()
            .find(|request| request.id == request_id)
        else {
            return Ok(None);
        };
        if expected_session_id.is_some()
            && Some(request.session_id.as_str()) != expected_session_id
        {
            return Ok(None);
        }
        if request.expires_at_ms > 0 && now_ms_u64() >= request.expires_at_ms {
            anyhow::bail!("QUESTION_REQUEST_EXPIRED");
        }
        let digest_payload = json!({
            "tenant": &request.tenant_context,
            "sessionID": &request.session_id,
            "questions": &request.questions,
            "tool": &request.tool,
        });
        let expected_digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&digest_payload).unwrap_or_default())
        );
        if request.action_digest.is_empty() {
            if !tenant_context.is_local_implicit() {
                anyhow::bail!("QUESTION_REQUEST_UNBOUND");
            }
        } else if request.action_digest != expected_digest {
            anyhow::bail!("QUESTION_REQUEST_ACTION_MISMATCH");
        }
        Ok(Some(request))
    }

    pub async fn decide_question_for_tenant(
        &self,
        request_id: &str,
        tenant_context: &TenantContext,
        expected_session_id: Option<&str>,
    ) -> anyhow::Result<Option<QuestionRequest>> {
        let _write_guard = self.question_write_lock.lock().await;
        let Some(request) = self
            .get_question_request_for_tenant(request_id, tenant_context, expected_session_id)
            .await?
        else {
            return Ok(None);
        };
        let request_id = request_id.to_string();
        let removed = self
            .run_blocking(move |repository| repository.remove_question(&request_id))
            .await?;
        Ok(removed.then_some(request))
    }

    pub async fn reply_question(&self, request_id: &str) -> anyhow::Result<bool> {
        let _write_guard = self.question_write_lock.lock().await;
        let request_id = request_id.to_string();
        self.run_blocking(move |repository| repository.remove_question(&request_id))
            .await
    }

    pub async fn reject_question(&self, request_id: &str) -> anyhow::Result<bool> {
        self.reply_question(request_id).await
    }

    pub async fn attach_session_to_workspace(
        &self,
        session_id: &str,
        target_workspace: &str,
        reason_tag: &str,
    ) -> anyhow::Result<Option<Session>> {
        let Some(target_workspace) = normalize_workspace_path(target_workspace) else {
            return Ok(None);
        };
        let session_id = session_id.to_string();
        let reason_tag = reason_tag.to_string();
        let project_id = workspace_project_id(&target_workspace);
        self.run_blocking(move |repository| {
            repository.attach_to_workspace(&session_id, &target_workspace, &reason_tag, project_id)
        }).await
    }

    async fn run_blocking<T, F>(&self, operation: F) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(session_repository::SessionRepository) -> anyhow::Result<T> + Send + 'static,
    {
        let repository = self.repository.clone();
        task::spawn_blocking(move || operation(repository))
            .await
            .context("session store task failed")?
    }
}

fn collect_legacy_import_state(
    base: &Path,
) -> anyhow::Result<session_repository::LegacyImportState> {
    let sessions_file = base.join("sessions.json");
    let metadata_file = base.join("session_meta.json");
    let questions_file = base.join("questions.json");
    // Import and source recording must use the same byte snapshot. A later
    // edit to one of these legacy files cannot create a mismatched digest.
    let source_bytes = [sessions_file.clone(), metadata_file.clone(), questions_file.clone()]
        .into_iter()
        .map(|path| {
            let bytes = if path.exists() {
                Some(
                    std::fs::read(&path)
                        .with_context(|| format!("failed to read {}", path.display()))?,
                )
            } else {
                None
            };
            Ok::<_, anyhow::Error>((path, bytes))
        })
        .collect::<anyhow::Result<HashMap<_, _>>>()?;

    let mut sessions = if let Some(raw) = source_bytes
        .get(&sessions_file)
        .and_then(|bytes| bytes.as_deref())
    {
        load_sessions_file(std::str::from_utf8(raw).context("sessions.json must be UTF-8")?)?.0
    } else {
        scan_legacy_sessions(base)?.sessions
    };
    hydrate_workspace_roots(&mut sessions);
    repair_session_titles(&mut sessions);

    let mut metadata = if let Some(raw) = source_bytes
        .get(&metadata_file)
        .and_then(|bytes| bytes.as_deref())
    {
        match serde_json::from_slice(raw) {
            Ok(metadata) => metadata,
            Err(error) => {
                tracing::warn!(
                    path = %metadata_file.display(),
                    %error,
                    "ignoring malformed optional legacy session metadata sidecar"
                );
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };
    compact_session_metadata(&sessions, &mut metadata);

    let questions = if let Some(raw) = source_bytes
        .get(&questions_file)
        .and_then(|bytes| bytes.as_deref())
    {
        match serde_json::from_slice(raw) {
            Ok(questions) => questions,
            Err(error) => {
                tracing::warn!(
                    path = %questions_file.display(),
                    %error,
                    "ignoring malformed optional legacy question sidecar"
                );
                HashMap::new()
            }
        }
    } else {
        HashMap::new()
    };

    let sources = [sessions_file, metadata_file, questions_file]
        .into_iter()
        .filter_map(|path| {
            source_bytes
                .get(&path)
                .and_then(|bytes| bytes.as_ref())
                .map(|bytes| session_repository::LegacySource {
                    digest: Some(format!("{:x}", Sha256::digest(bytes))),
                    path,
                })
        })
        .collect();
    Ok(session_repository::LegacyImportState {
        sessions,
        metadata,
        questions,
        sources,
    })
}

async fn sync_temp_storage_file(path: &Path) -> anyhow::Result<()> {
    let std_path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut last_error = None;
        for attempt in 0..5 {
            match std::fs::File::open(&std_path).and_then(|file| file.sync_all()) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    last_error = Some(error);
                    std::thread::sleep(std::time::Duration::from_millis(25 * (attempt + 1)));
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::Other, "temp storage file sync failed")
        }))
    })
    .await
    .context("temp storage file sync task failed")?
    .with_context(|| format!("failed to sync temp storage file {}", path.display()))
}

async fn commit_temp_file(temp_path: &Path, path: &Path) -> std::io::Result<()> {
    match tokio::fs::rename(temp_path, path).await {
        Ok(()) => Ok(()),
        Err(err) => {
            #[cfg(windows)]
            {
                // Windows `rename` can return PermissionDenied when replacing an existing file.
                // Fall back to delete-then-rename for this case.
                use std::io::ErrorKind;
                if matches!(
                    err.kind(),
                    ErrorKind::PermissionDenied | ErrorKind::AlreadyExists
                ) {
                    let mut last_err = err;
                    for attempt in 0..5 {
                        match tokio::fs::remove_file(path).await {
                            Ok(()) => {}
                            Err(remove_err) if remove_err.kind() == ErrorKind::NotFound => {}
                            Err(remove_err) => {
                                last_err = remove_err;
                                tokio::time::sleep(std::time::Duration::from_millis(
                                    25 * (attempt + 1),
                                ))
                                .await;
                                continue;
                            }
                        }
                        match tokio::fs::rename(temp_path, path).await {
                            Ok(()) => return Ok(()),
                            Err(rename_err) => {
                                last_err = rename_err;
                                tokio::time::sleep(std::time::Duration::from_millis(
                                    25 * (attempt + 1),
                                ))
                                .await;
                            }
                        }
                    }
                    return Err(last_err);
                }
            }
            Err(err)
        }
    }
}

fn normalize_todo_items(items: Vec<Value>) -> Vec<Value> {
    items
        .into_iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let content = obj
                .get("content")
                .and_then(|v| v.as_str())
                .or_else(|| obj.get("text").and_then(|v| v.as_str()))
                .unwrap_or("")
                .trim()
                .to_string();
            if content.is_empty() {
                return None;
            }
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("todo-{}", Uuid::new_v4()));
            let status = obj
                .get("status")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| "pending".to_string());
            Some(json!({
                "id": id,
                "content": content,
                "status": status
            }))
        })
        .collect()
}

#[derive(Debug)]
struct LegacyScanResult {
    sessions: HashMap<String, Session>,
    legacy_counts: LegacyTreeCounts,
    imported_counts: LegacyImportedCounts,
}

#[derive(Debug, Default)]
struct LegacyMergeStats {
    changed: bool,
    sessions_merged: u64,
    messages_recovered: u64,
    parts_recovered: u64,
}

fn now_ms_u64() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

async fn should_run_legacy_scan_on_startup(marker_path: &Path, sessions_exist: bool) -> bool {
    if !sessions_exist {
        return true;
    }
    // Fast-path startup: if canonical sessions already exist, do not block startup
    // on deep legacy tree scans. Users can trigger an explicit repair scan later.
    if read_legacy_import_marker(marker_path).await.is_none() {
        return false;
    }
    false
}

async fn read_legacy_import_marker(marker_path: &Path) -> Option<LegacyImportMarker> {
    let raw = fs::read_to_string(marker_path).await.ok()?;
    serde_json::from_str::<LegacyImportMarker>(&raw).ok()
}

fn scan_legacy_sessions(base: &Path) -> anyhow::Result<LegacyScanResult> {
    let sessions = load_legacy_opencode_sessions(base).unwrap_or_default();
    let imported_counts = LegacyImportedCounts {
        sessions: sessions.len() as u64,
        messages: sessions.values().map(|s| s.messages.len() as u64).sum(),
        parts: sessions
            .values()
            .flat_map(|s| s.messages.iter())
            .map(|m| m.parts.len() as u64)
            .sum(),
    };
    let legacy_counts = LegacyTreeCounts {
        session_files: count_legacy_json_files(&base.join("session")),
        message_files: count_legacy_json_files(&base.join("message")),
        part_files: count_legacy_json_files(&base.join("part")),
    };
    Ok(LegacyScanResult {
        sessions,
        legacy_counts,
        imported_counts,
    })
}

fn count_legacy_json_files(root: &Path) -> u64 {
    if !root.is_dir() {
        return 0;
    }
    let mut count = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                    count += 1;
                }
            }
        }
    }
    count
}

fn merge_legacy_sessions(
    current: &mut HashMap<String, Session>,
    imported: HashMap<String, Session>,
) -> bool {
    merge_legacy_sessions_with_stats(current, imported).changed
}

fn merge_legacy_sessions_with_stats(
    current: &mut HashMap<String, Session>,
    imported: HashMap<String, Session>,
) -> LegacyMergeStats {
    let mut stats = LegacyMergeStats::default();
    for (id, legacy) in imported {
        let legacy_message_count = legacy.messages.len() as u64;
        let legacy_part_count = legacy
            .messages
            .iter()
            .map(|m| m.parts.len() as u64)
            .sum::<u64>();
        match current.get_mut(&id) {
            None => {
                current.insert(id, legacy);
                stats.changed = true;
                stats.sessions_merged += 1;
                stats.messages_recovered += legacy_message_count;
                stats.parts_recovered += legacy_part_count;
            }
            Some(existing) => {
                let should_merge_messages =
                    existing.messages.is_empty() && !legacy.messages.is_empty();
                let should_fill_title =
                    existing.title.trim().is_empty() && !legacy.title.trim().is_empty();
                let should_fill_directory = (existing.directory.trim().is_empty()
                    || existing.directory.trim() == "."
                    || existing.directory.trim() == "./"
                    || existing.directory.trim() == ".\\")
                    && !legacy.directory.trim().is_empty();
                let should_fill_workspace =
                    existing.workspace_root.is_none() && legacy.workspace_root.is_some();
                if should_merge_messages {
                    existing.messages = legacy.messages.clone();
                }
                if should_fill_title {
                    existing.title = legacy.title.clone();
                }
                if should_fill_directory {
                    existing.directory = legacy.directory.clone();
                }
                if should_fill_workspace {
                    existing.workspace_root = legacy.workspace_root.clone();
                }
                if should_merge_messages
                    || should_fill_title
                    || should_fill_directory
                    || should_fill_workspace
                {
                    stats.changed = true;
                    if should_merge_messages {
                        stats.sessions_merged += 1;
                        stats.messages_recovered += legacy_message_count;
                        stats.parts_recovered += legacy_part_count;
                    }
                }
            }
        }
    }
    stats
}

fn hydrate_workspace_roots(sessions: &mut HashMap<String, Session>) -> bool {
    let mut changed = false;
    for session in sessions.values_mut() {
        if session.workspace_root.is_none() {
            let normalized = normalize_workspace_path(&session.directory);
            if normalized.is_some() {
                session.workspace_root = normalized;
                changed = true;
            }
        }
    }
    changed
}

fn repair_session_titles(sessions: &mut HashMap<String, Session>) -> bool {
    let mut changed = false;
    for session in sessions.values_mut() {
        if !title_needs_repair(&session.title) {
            continue;
        }
        let first_user_text = session.messages.iter().find_map(|message| {
            if !matches!(message.role, MessageRole::User) {
                return None;
            }
            message.parts.iter().find_map(|part| match part {
                MessagePart::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
                _ => None,
            })
        });
        let Some(source) = first_user_text else {
            continue;
        };
        let Some(derived) = derive_session_title_from_prompt(source, 60) else {
            continue;
        };
        if derived == session.title {
            continue;
        }
        session.title = derived;
        session.time.updated = Utc::now();
        changed = true;
    }
    changed
}

#[derive(Debug, Deserialize)]
struct LegacySessionTime {
    created: i64,
    updated: i64,
}

#[derive(Debug, Deserialize)]
struct LegacySession {
    id: String,
    slug: Option<String>,
    version: Option<String>,
    #[serde(rename = "projectID")]
    project_id: Option<String>,
    title: Option<String>,
    directory: Option<String>,
    time: LegacySessionTime,
}

fn load_legacy_opencode_sessions(base: &Path) -> anyhow::Result<HashMap<String, Session>> {
    let legacy_root = base.join("session");
    if !legacy_root.is_dir() {
        return Ok(HashMap::new());
    }

    let mut out = HashMap::new();
    let mut stack = vec![legacy_root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let raw = match std::fs::read_to_string(&path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let legacy = match serde_json::from_str::<LegacySession>(&raw) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let created = Utc
                .timestamp_millis_opt(legacy.time.created)
                .single()
                .unwrap_or_else(Utc::now);
            let updated = Utc
                .timestamp_millis_opt(legacy.time.updated)
                .single()
                .unwrap_or(created);

            let session_id = legacy.id.clone();
            out.insert(
                session_id.clone(),
                Session {
                    id: session_id.clone(),
                    slug: legacy.slug,
                    version: legacy.version,
                    project_id: legacy.project_id,
                    title: legacy
                        .title
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| "New session".to_string()),
                    directory: legacy
                        .directory
                        .clone()
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or_else(|| ".".to_string()),
                    workspace_root: legacy
                        .directory
                        .as_deref()
                        .and_then(normalize_workspace_path),
                    pinned_workspace_id: None,
                    origin_workspace_root: None,
                    attached_from_workspace: None,
                    attached_to_workspace: None,
                    attach_timestamp_ms: None,
                    attach_reason: None,
                    tenant_context: tandem_types::LocalImplicitTenant.into(),
                    verified_tenant_context: None,
                    time: tandem_types::SessionTime { created, updated },
                    model: None,
                    provider: None,
                    sampling: tandem_types::SamplingParams::default(),
                    source_kind: None,
                    source_metadata: None,
                    environment: None,
                    messages: load_legacy_session_messages(base, &session_id),
                },
            );
        }
    }
    Ok(out)
}

#[derive(Debug, Deserialize)]
struct LegacyMessageTime {
    created: i64,
}

#[derive(Debug, Deserialize)]
struct LegacyMessage {
    id: String,
    role: String,
    time: LegacyMessageTime,
}

#[derive(Debug, Deserialize)]
struct LegacyPart {
    #[serde(rename = "type")]
    part_type: Option<String>,
    text: Option<String>,
    tool: Option<String>,
    args: Option<Value>,
    result: Option<Value>,
    error: Option<String>,
}

fn load_legacy_session_messages(base: &Path, session_id: &str) -> Vec<Message> {
    let msg_dir = base.join("message").join(session_id);
    if !msg_dir.is_dir() {
        return Vec::new();
    }

    let mut legacy_messages: Vec<(i64, Message)> = Vec::new();

    let Ok(entries) = std::fs::read_dir(&msg_dir) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(legacy) = serde_json::from_str::<LegacyMessage>(&raw) else {
            continue;
        };

        let created_at = Utc
            .timestamp_millis_opt(legacy.time.created)
            .single()
            .unwrap_or_else(Utc::now);

        legacy_messages.push((
            legacy.time.created,
            Message {
                id: legacy.id.clone(),
                role: legacy_role_to_message_role(&legacy.role),
                parts: load_legacy_message_parts(base, &legacy.id),
                created_at,
            },
        ));
    }

    legacy_messages.sort_by_key(|(created_ms, _)| *created_ms);
    legacy_messages.into_iter().map(|(_, msg)| msg).collect()
}

fn load_legacy_message_parts(base: &Path, message_id: &str) -> Vec<MessagePart> {
    let parts_dir = base.join("part").join(message_id);
    if !parts_dir.is_dir() {
        return Vec::new();
    }

    let Ok(entries) = std::fs::read_dir(&parts_dir) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(part) = serde_json::from_str::<LegacyPart>(&raw) else {
            continue;
        };

        let mapped = if let Some(tool) = part.tool {
            Some(MessagePart::ToolInvocation {
                tool,
                args: part.args.unwrap_or_else(|| json!({})),
                result: part.result,
                error: part.error,
            })
        } else {
            match part.part_type.as_deref() {
                Some("reasoning") => Some(MessagePart::Reasoning {
                    text: part.text.unwrap_or_default(),
                }),
                Some("tool") => Some(MessagePart::ToolInvocation {
                    tool: "tool".to_string(),
                    args: part.args.unwrap_or_else(|| json!({})),
                    result: part.result,
                    error: part.error,
                }),
                Some("text") | None => Some(MessagePart::Text {
                    text: part.text.unwrap_or_default(),
                }),
                _ => None,
            }
        };

        if let Some(part) = mapped {
            out.push(part);
        }
    }
    out
}

fn legacy_role_to_message_role(role: &str) -> MessageRole {
    match role.to_lowercase().as_str() {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "system" => MessageRole::System,
        "tool" => MessageRole::Tool,
        _ => MessageRole::Assistant,
    }
}

#[derive(Debug, Clone, Default)]
struct MessageMergeStats {
    messages_recovered: u64,
    parts_recovered: u64,
    conflicts_merged: u64,
}

fn message_richness(msg: &Message) -> usize {
    msg.parts
        .iter()
        .map(|p| match p {
            MessagePart::Text { text } | MessagePart::Reasoning { text } => {
                if text.trim().is_empty() {
                    0
                } else {
                    1
                }
            }
            MessagePart::ToolInvocation { result, error, .. } => {
                if result.is_some() || error.is_some() {
                    2
                } else {
                    1
                }
            }
        })
        .sum()
}

fn most_recent_message_time(messages: &[Message]) -> Option<chrono::DateTime<Utc>> {
    messages.iter().map(|m| m.created_at).max()
}

fn merge_session_messages(
    existing: &[Message],
    imported: &[Message],
) -> (Vec<Message>, MessageMergeStats, bool) {
    if existing.is_empty() {
        let messages_recovered = imported.len() as u64;
        let parts_recovered = imported.iter().map(|m| m.parts.len() as u64).sum();
        return (
            imported.to_vec(),
            MessageMergeStats {
                messages_recovered,
                parts_recovered,
                conflicts_merged: 0,
            },
            true,
        );
    }

    let mut merged_by_id: HashMap<String, Message> = existing
        .iter()
        .cloned()
        .map(|m| (m.id.clone(), m))
        .collect();
    let mut stats = MessageMergeStats::default();
    let mut changed = false;

    for incoming in imported {
        match merged_by_id.get(&incoming.id) {
            None => {
                merged_by_id.insert(incoming.id.clone(), incoming.clone());
                stats.messages_recovered += 1;
                stats.parts_recovered += incoming.parts.len() as u64;
                changed = true;
            }
            Some(current) => {
                let incoming_richer = message_richness(incoming) > message_richness(current)
                    || incoming.parts.len() > current.parts.len();
                if incoming_richer {
                    merged_by_id.insert(incoming.id.clone(), incoming.clone());
                    stats.conflicts_merged += 1;
                    changed = true;
                }
            }
        }
    }

    let mut out: Vec<Message> = merged_by_id.into_values().collect();
    out.sort_by_key(|m| m.created_at);
    (out, stats, changed)
}
