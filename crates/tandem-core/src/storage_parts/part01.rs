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
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(default)]
    pub questions: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<QuestionToolRef>,
}

pub struct Storage {
    base: PathBuf,
    sessions: RwLock<HashMap<String, Session>>,
    metadata: RwLock<HashMap<String, SessionMeta>>,
    question_requests: RwLock<HashMap<String, QuestionRequest>>,
    flush_lock: Mutex<()>,
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
        let sessions_file = base.join("sessions.json");
        let marker_path = base.join(LEGACY_IMPORT_MARKER_FILE);
        let sessions_file_exists = sessions_file.exists();
        let mut imported_legacy_sessions = false;
        let mut sessions = if sessions_file_exists {
            let raw = fs::read_to_string(&sessions_file).await?;
            serde_json::from_str::<HashMap<String, Session>>(&raw).unwrap_or_default()
        } else {
            HashMap::new()
        };

        let mut marker_to_write = None;
        if should_run_legacy_scan_on_startup(&marker_path, sessions_file_exists).await {
            let base_for_scan = base.clone();
            let scan = task::spawn_blocking(move || scan_legacy_sessions(&base_for_scan))
                .await
                .map_err(|err| anyhow::anyhow!("legacy scan task join error: {}", err))??;
            if merge_legacy_sessions(&mut sessions, scan.sessions) {
                imported_legacy_sessions = true;
            }
            marker_to_write = Some(LegacyImportMarker {
                version: LEGACY_IMPORT_MARKER_VERSION,
                created_at_ms: now_ms_u64(),
                last_checked_at_ms: now_ms_u64(),
                legacy_counts: scan.legacy_counts,
                imported_counts: scan.imported_counts,
            });
        }

        if hydrate_workspace_roots(&mut sessions) {
            imported_legacy_sessions = true;
        }
        if repair_session_titles(&mut sessions) {
            imported_legacy_sessions = true;
        }
        let metadata_file = base.join("session_meta.json");
        let mut metadata = if metadata_file.exists() {
            let raw = fs::read_to_string(&metadata_file).await?;
            serde_json::from_str::<HashMap<String, SessionMeta>>(&raw).unwrap_or_default()
        } else {
            HashMap::new()
        };
        let compaction = compact_session_metadata(&sessions, &mut metadata);
        let metadata_compacted = compaction.metadata_pruned > 0 || compaction.snapshots_removed > 0;
        if metadata_compacted {
            tracing::info!(
                metadata_pruned = compaction.metadata_pruned,
                snapshots_removed = compaction.snapshots_removed,
                "compacted persisted session metadata"
            );
        }
        let questions_file = base.join("questions.json");
        let question_requests = if questions_file.exists() {
            let raw = fs::read_to_string(&questions_file).await?;
            serde_json::from_str::<HashMap<String, QuestionRequest>>(&raw).unwrap_or_default()
        } else {
            HashMap::new()
        };
        let storage = Self {
            base,
            sessions: RwLock::new(sessions),
            metadata: RwLock::new(metadata),
            question_requests: RwLock::new(question_requests),
            flush_lock: Mutex::new(()),
        };

        if imported_legacy_sessions || metadata_compacted {
            storage.flush().await?;
        }
        if let Some(marker) = marker_to_write {
            storage.write_legacy_import_marker(&marker).await?;
        }

        Ok(storage)
    }

    pub async fn list_sessions(&self) -> Vec<Session> {
        self.list_sessions_scoped(SessionListScope::Global).await
    }

    pub async fn list_sessions_scoped(&self, scope: SessionListScope) -> Vec<Session> {
        let all = self
            .sessions
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        match scope {
            SessionListScope::Global => all,
            SessionListScope::Workspace { workspace_root } => {
                let Some(normalized_workspace) = normalize_workspace_path(&workspace_root) else {
                    return Vec::new();
                };
                all.into_iter()
                    .filter(|session| {
                        let direct = session
                            .workspace_root
                            .as_ref()
                            .and_then(|p| normalize_workspace_path(p))
                            .map(|p| p == normalized_workspace)
                            .unwrap_or(false);
                        if direct {
                            return true;
                        }
                        normalize_workspace_path(&session.directory)
                            .map(|p| p == normalized_workspace)
                            .unwrap_or(false)
                    })
                    .collect()
            }
        }
    }

    pub async fn get_session(&self, id: &str) -> Option<Session> {
        self.sessions.read().await.get(id).cloned()
    }

    pub async fn save_session(&self, mut session: Session) -> anyhow::Result<()> {
        if session.workspace_root.is_none() {
            session.workspace_root = normalize_workspace_path(&session.directory);
        }
        let session_id = session.id.clone();
        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session);
        self.metadata
            .write()
            .await
            .entry(session_id)
            .or_insert_with(SessionMeta::default);
        self.flush().await
    }

    pub async fn repair_sessions_from_file_store(&self) -> anyhow::Result<SessionRepairStats> {
        let mut stats = SessionRepairStats::default();
        let mut sessions = self.sessions.write().await;

        for session in sessions.values_mut() {
            let imported = load_legacy_session_messages(&self.base, &session.id);
            if imported.is_empty() {
                continue;
            }

            let (merged, merge_stats, changed) =
                merge_session_messages(&session.messages, &imported);
            if changed {
                session.messages = merged;
                session.time.updated =
                    most_recent_message_time(&session.messages).unwrap_or(session.time.updated);
                stats.sessions_repaired += 1;
                stats.messages_recovered += merge_stats.messages_recovered;
                stats.parts_recovered += merge_stats.parts_recovered;
                stats.conflicts_merged += merge_stats.conflicts_merged;
            }
        }

        if stats.sessions_repaired > 0 {
            drop(sessions);
            self.flush().await?;
        }

        Ok(stats)
    }

    pub async fn run_legacy_storage_repair_scan(
        &self,
        force: bool,
    ) -> anyhow::Result<LegacyRepairRunReport> {
        let marker_path = self.base.join(LEGACY_IMPORT_MARKER_FILE);
        let sessions_exists = self.base.join("sessions.json").exists();
        let should_scan = if force {
            true
        } else {
            should_run_legacy_scan_on_startup(&marker_path, sessions_exists).await
        };
        if !should_scan {
            let marker = read_legacy_import_marker(&marker_path)
                .await
                .unwrap_or_else(|| LegacyImportMarker {
                    version: LEGACY_IMPORT_MARKER_VERSION,
                    created_at_ms: now_ms_u64(),
                    last_checked_at_ms: now_ms_u64(),
                    legacy_counts: LegacyTreeCounts::default(),
                    imported_counts: LegacyImportedCounts::default(),
                });
            return Ok(LegacyRepairRunReport {
                status: "skipped".to_string(),
                marker_updated: false,
                sessions_merged: 0,
                messages_recovered: 0,
                parts_recovered: 0,
                legacy_counts: marker.legacy_counts,
                imported_counts: marker.imported_counts,
            });
        }

        let base_for_scan = self.base.clone();
        let scan = task::spawn_blocking(move || scan_legacy_sessions(&base_for_scan))
            .await
            .map_err(|err| anyhow::anyhow!("legacy scan task join error: {}", err))??;

        let merge_stats = {
            let mut sessions = self.sessions.write().await;
            merge_legacy_sessions_with_stats(&mut sessions, scan.sessions)
        };

        if merge_stats.changed {
            self.flush().await?;
        }

        let marker = LegacyImportMarker {
            version: LEGACY_IMPORT_MARKER_VERSION,
            created_at_ms: now_ms_u64(),
            last_checked_at_ms: now_ms_u64(),
            legacy_counts: scan.legacy_counts.clone(),
            imported_counts: scan.imported_counts.clone(),
        };
        self.write_legacy_import_marker(&marker).await?;

        Ok(LegacyRepairRunReport {
            status: if merge_stats.changed {
                "updated".to_string()
            } else {
                "no_changes".to_string()
            },
            marker_updated: true,
            sessions_merged: merge_stats.sessions_merged,
            messages_recovered: merge_stats.messages_recovered,
            parts_recovered: merge_stats.parts_recovered,
            legacy_counts: scan.legacy_counts,
            imported_counts: scan.imported_counts,
        })
    }

    pub async fn delete_session(&self, id: &str) -> anyhow::Result<bool> {
        let removed = self.sessions.write().await.remove(id).is_some();
        self.metadata.write().await.remove(id);
        self.question_requests
            .write()
            .await
            .retain(|_, request| request.session_id != id);
        if removed {
            self.flush().await?;
        }
        Ok(removed)
    }

    pub async fn append_message(&self, session_id: &str, msg: Message) -> anyhow::Result<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(session_id)
            .context("session not found for append_message")?;
        session.messages.push(msg);
        session.time.updated = Utc::now();
        drop(sessions);
        self.flush().await
    }

    pub async fn append_message_part(
        &self,
        session_id: &str,
        message_id: &str,
        part: MessagePart,
    ) -> anyhow::Result<()> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(session_id)
            .context("session not found for append_message_part")?;
        let message = if let Some(message) = session
            .messages
            .iter_mut()
            .find(|message| message.id == message_id)
        {
            message
        } else {
            session
                .messages
                .iter_mut()
                .rev()
                .find(|message| matches!(message.role, MessageRole::User))
                .context("message not found for append_message_part")?
        };
        reduce_message_parts(&mut message.parts, part);
        session.time.updated = Utc::now();
        drop(sessions);
        self.flush().await
    }

    pub async fn fork_session(&self, id: &str) -> anyhow::Result<Option<Session>> {
        let source = {
            let sessions = self.sessions.read().await;
            sessions.get(id).cloned()
        };
        let Some(mut child) = source else {
            return Ok(None);
        };

        child.id = Uuid::new_v4().to_string();
        child.title = format!("{} (fork)", child.title);
        child.time.created = Utc::now();
        child.time.updated = child.time.created;
        child.slug = None;

        self.sessions
            .write()
            .await
            .insert(child.id.clone(), child.clone());
        self.metadata.write().await.insert(
            child.id.clone(),
            SessionMeta {
                parent_id: Some(id.to_string()),
                snapshots: vec![child.messages.clone()],
                ..SessionMeta::default()
            },
        );
        self.flush().await?;
        Ok(Some(child))
    }

    pub async fn revert_session(&self, id: &str) -> anyhow::Result<bool> {
        let mut sessions = self.sessions.write().await;
        let Some(session) = sessions.get_mut(id) else {
            return Ok(false);
        };
        let mut metadata = self.metadata.write().await;
        let meta = metadata
            .entry(id.to_string())
            .or_insert_with(SessionMeta::default);
        let Some(snapshot) = meta.snapshots.pop() else {
            return Ok(false);
        };
        meta.pre_revert = Some(session.messages.clone());
        session.messages = snapshot;
        session.time.updated = Utc::now();
        drop(metadata);
        drop(sessions);
        self.flush().await?;
        Ok(true)
    }

    pub async fn unrevert_session(&self, id: &str) -> anyhow::Result<bool> {
        let mut sessions = self.sessions.write().await;
        let Some(session) = sessions.get_mut(id) else {
            return Ok(false);
        };
        let mut metadata = self.metadata.write().await;
        let Some(meta) = metadata.get_mut(id) else {
            return Ok(false);
        };
        let Some(previous) = meta.pre_revert.take() else {
            return Ok(false);
        };
        meta.snapshots.push(session.messages.clone());
        trim_session_snapshots(&mut meta.snapshots);
        session.messages = previous;
        session.time.updated = Utc::now();
        drop(metadata);
        drop(sessions);
        self.flush().await?;
        Ok(true)
    }

    pub async fn set_shared(&self, id: &str, shared: bool) -> anyhow::Result<Option<String>> {
        let mut metadata = self.metadata.write().await;
        let meta = metadata
            .entry(id.to_string())
            .or_insert_with(SessionMeta::default);
        meta.shared = shared;
        if shared {
            if meta.share_id.is_none() {
                meta.share_id = Some(Uuid::new_v4().to_string());
            }
        } else {
            meta.share_id = None;
        }
        let share_id = meta.share_id.clone();
        drop(metadata);
        self.flush().await?;
        Ok(share_id)
    }

    pub async fn set_archived(&self, id: &str, archived: bool) -> anyhow::Result<bool> {
        let mut metadata = self.metadata.write().await;
        let meta = metadata
            .entry(id.to_string())
            .or_insert_with(SessionMeta::default);
        meta.archived = archived;
        drop(metadata);
        self.flush().await?;
        Ok(true)
    }

    pub async fn set_summary(&self, id: &str, summary: String) -> anyhow::Result<bool> {
        let mut metadata = self.metadata.write().await;
        let meta = metadata
            .entry(id.to_string())
            .or_insert_with(SessionMeta::default);
        meta.summary = Some(summary);
        drop(metadata);
        self.flush().await?;
        Ok(true)
    }

    pub async fn children(&self, parent_id: &str) -> Vec<Session> {
        let child_ids = {
            let metadata = self.metadata.read().await;
            metadata
                .iter()
                .filter(|(_, meta)| meta.parent_id.as_deref() == Some(parent_id))
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        let sessions = self.sessions.read().await;
        child_ids
            .into_iter()
            .filter_map(|id| sessions.get(&id).cloned())
            .collect()
    }

    pub async fn session_status(&self, id: &str) -> Option<Value> {
        let metadata = self.metadata.read().await;
        metadata.get(id).map(|meta| {
            json!({
                "archived": meta.archived,
                "shared": meta.shared,
                "parentID": meta.parent_id,
                "snapshotCount": meta.snapshots.len()
            })
        })
    }

    pub async fn session_diff(&self, id: &str) -> Option<Value> {
        let sessions = self.sessions.read().await;
        let current = sessions.get(id)?;
        let metadata = self.metadata.read().await;
        let default = SessionMeta::default();
        let meta = metadata.get(id).unwrap_or(&default);
        let last_snapshot_len = meta.snapshots.last().map(|s| s.len()).unwrap_or(0);
        Some(json!({
            "sessionID": id,
            "currentMessageCount": current.messages.len(),
            "lastSnapshotMessageCount": last_snapshot_len,
            "delta": current.messages.len() as i64 - last_snapshot_len as i64
        }))
    }

    pub async fn set_todos(&self, id: &str, todos: Vec<Value>) -> anyhow::Result<()> {
        let mut metadata = self.metadata.write().await;
        let meta = metadata
            .entry(id.to_string())
            .or_insert_with(SessionMeta::default);
        meta.todos = normalize_todo_items(todos);
        drop(metadata);
        self.flush().await
    }

    pub async fn get_todos(&self, id: &str) -> Vec<Value> {
        let todos = self
            .metadata
            .read()
            .await
            .get(id)
            .map(|meta| meta.todos.clone())
            .unwrap_or_default();
        normalize_todo_items(todos)
    }

    pub async fn add_question_request(
        &self,
        session_id: &str,
        message_id: &str,
        questions: Vec<Value>,
    ) -> anyhow::Result<QuestionRequest> {
        if questions.is_empty() {
            return Err(anyhow::anyhow!(
                "cannot add empty question request for session {}",
                session_id
            ));
        }
        let request = QuestionRequest {
            id: format!("q-{}", Uuid::new_v4()),
            session_id: session_id.to_string(),
            questions,
            tool: Some(QuestionToolRef {
                call_id: format!("call-{}", Uuid::new_v4()),
                message_id: message_id.to_string(),
            }),
        };
        self.question_requests
            .write()
            .await
            .insert(request.id.clone(), request.clone());
        self.flush().await?;
        Ok(request)
    }

    pub async fn list_question_requests(&self) -> Vec<QuestionRequest> {
        self.question_requests
            .read()
            .await
            .values()
            .cloned()
            .collect()
    }

    pub async fn reply_question(&self, request_id: &str) -> anyhow::Result<bool> {
        let removed = self
            .question_requests
            .write()
            .await
            .remove(request_id)
            .is_some();
        if removed {
            self.flush().await?;
        }
        Ok(removed)
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
        let mut sessions = self.sessions.write().await;
        let Some(session) = sessions.get_mut(session_id) else {
            return Ok(None);
        };
        let previous_workspace = session
            .workspace_root
            .clone()
            .or_else(|| normalize_workspace_path(&session.directory));

        if session.origin_workspace_root.is_none() {
            session.origin_workspace_root = previous_workspace.clone();
        }
        session.attached_from_workspace = previous_workspace;
        session.attached_to_workspace = Some(target_workspace.clone());
        session.attach_timestamp_ms = Some(Utc::now().timestamp_millis().max(0) as u64);
        session.attach_reason = Some(reason_tag.trim().to_string());
        session.workspace_root = Some(target_workspace.clone());
        session.project_id = workspace_project_id(&target_workspace);
        session.directory = target_workspace;
        session.time.updated = Utc::now();
        let updated = session.clone();
        drop(sessions);
        self.flush().await?;
        Ok(Some(updated))
    }

    async fn flush(&self) -> anyhow::Result<()> {
        let _flush_guard = self.flush_lock.lock().await;
        {
            let snapshot = self.sessions.read().await.clone();
            self.flush_file("sessions.json", &snapshot).await?;
        }
        {
            let metadata_snapshot = self.metadata.read().await.clone();
            self.flush_file("session_meta.json", &metadata_snapshot)
                .await?;
        }
        {
            let questions_snapshot = self.question_requests.read().await.clone();
            self.flush_file("questions.json", &questions_snapshot)
                .await?;
        }
        Ok(())
    }

    async fn flush_file(&self, filename: &str, data: &impl serde::Serialize) -> anyhow::Result<()> {
        let path = self.base.join(filename);
        let temp_path = self.base.join(format!("{}.tmp", filename));
        let payload = serde_json::to_string_pretty(data)?;
        fs::write(&temp_path, payload).await.with_context(|| {
            format!("failed to write temp storage file {}", temp_path.display())
        })?;
        let std_temp_path: std::path::PathBuf = temp_path.clone().try_into()?;
        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&std_temp_path)?;
            file.sync_all()?;
            Ok::<(), std::io::Error>(())
        })
        .await??;
        commit_temp_file(&temp_path, &path).await.with_context(|| {
            format!(
                "failed to atomically replace storage file {} with {}",
                path.display(),
                temp_path.display()
            )
        })?;
        Ok(())
    }

    async fn write_legacy_import_marker(&self, marker: &LegacyImportMarker) -> anyhow::Result<()> {
        let payload = serde_json::to_string_pretty(marker)?;
        fs::write(self.base.join(LEGACY_IMPORT_MARKER_FILE), payload).await?;
        Ok(())
    }
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
                    match tokio::fs::remove_file(path).await {
                        Ok(()) => {}
                        Err(remove_err) if remove_err.kind() == ErrorKind::NotFound => {}
                        Err(remove_err) => return Err(remove_err),
                    }
                    return tokio::fs::rename(temp_path, path).await;
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
                    origin_workspace_root: None,
                    attached_from_workspace: None,
                    attached_to_workspace: None,
                    attach_timestamp_ms: None,
                    attach_reason: None,
                    tenant_context: tandem_types::LocalImplicitTenant.into(),
                    time: tandem_types::SessionTime { created, updated },
                    model: None,
                    provider: None,
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
