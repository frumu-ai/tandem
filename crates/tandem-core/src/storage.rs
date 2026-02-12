use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::fs;
use tokio::sync::RwLock;
use uuid::Uuid;

use tandem_types::{Message, Session};

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
}

impl Storage {
    pub async fn new(base: impl AsRef<Path>) -> anyhow::Result<Self> {
        let base = base.as_ref().to_path_buf();
        fs::create_dir_all(&base).await?;
        let sessions_file = base.join("sessions.json");
        let sessions = if sessions_file.exists() {
            let raw = fs::read_to_string(&sessions_file).await?;
            serde_json::from_str::<HashMap<String, Session>>(&raw).unwrap_or_default()
        } else {
            HashMap::new()
        };
        let metadata_file = base.join("session_meta.json");
        let metadata = if metadata_file.exists() {
            let raw = fs::read_to_string(&metadata_file).await?;
            serde_json::from_str::<HashMap<String, SessionMeta>>(&raw).unwrap_or_default()
        } else {
            HashMap::new()
        };
        let questions_file = base.join("questions.json");
        let question_requests = if questions_file.exists() {
            let raw = fs::read_to_string(&questions_file).await?;
            serde_json::from_str::<HashMap<String, QuestionRequest>>(&raw).unwrap_or_default()
        } else {
            HashMap::new()
        };
        Ok(Self {
            base,
            sessions: RwLock::new(sessions),
            metadata: RwLock::new(metadata),
            question_requests: RwLock::new(question_requests),
        })
    }

    pub async fn list_sessions(&self) -> Vec<Session> {
        self.sessions.read().await.values().cloned().collect()
    }

    pub async fn get_session(&self, id: &str) -> Option<Session> {
        self.sessions.read().await.get(id).cloned()
    }

    pub async fn save_session(&self, session: Session) -> anyhow::Result<()> {
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
        let mut meta_guard = self.metadata.write().await;
        let meta = meta_guard
            .entry(session_id.to_string())
            .or_insert_with(SessionMeta::default);
        meta.snapshots.push(session.messages.clone());
        if meta.snapshots.len() > 25 {
            let _ = meta.snapshots.remove(0);
        }
        session.messages.push(msg);
        session.time.updated = Utc::now();
        drop(sessions);
        drop(meta_guard);
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

    async fn flush(&self) -> anyhow::Result<()> {
        let snapshot = self.sessions.read().await.clone();
        let payload = serde_json::to_string_pretty(&snapshot)?;
        fs::write(self.base.join("sessions.json"), payload).await?;
        let metadata_snapshot = self.metadata.read().await.clone();
        let metadata_payload = serde_json::to_string_pretty(&metadata_snapshot)?;
        fs::write(self.base.join("session_meta.json"), metadata_payload).await?;
        let questions_snapshot = self.question_requests.read().await.clone();
        let questions_payload = serde_json::to_string_pretty(&questions_snapshot)?;
        fs::write(self.base.join("questions.json"), questions_payload).await?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn todos_are_normalized_to_wire_shape() {
        let base = std::env::temp_dir().join(format!("tandem-core-test-{}", Uuid::new_v4()));
        let storage = Storage::new(&base).await.expect("storage");
        let session = Session::new(Some("test".to_string()), Some(".".to_string()));
        let id = session.id.clone();
        storage.save_session(session).await.expect("save session");

        storage
            .set_todos(
                &id,
                vec![
                    json!({"content":"first"}),
                    json!({"text":"second", "status":"in_progress"}),
                    json!({"id":"keep-id","content":"third","status":"completed"}),
                ],
            )
            .await
            .expect("set todos");

        let todos = storage.get_todos(&id).await;
        assert_eq!(todos.len(), 3);
        for todo in todos {
            assert!(todo.get("id").and_then(|v| v.as_str()).is_some());
            assert!(todo.get("content").and_then(|v| v.as_str()).is_some());
            assert!(todo.get("status").and_then(|v| v.as_str()).is_some());
        }
    }
}
