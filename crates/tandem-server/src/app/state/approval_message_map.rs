use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tandem_channels::traits::InteractiveCardSent;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalMessageRecord {
    pub request_id: String,
    pub channel: String,
    pub recipient: String,
    pub message_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

impl ApprovalMessageRecord {
    pub fn from_sent(request_id: impl Into<String>, sent: InteractiveCardSent) -> Self {
        Self {
            request_id: request_id.into(),
            channel: sent.channel,
            recipient: sent.recipient,
            message_id: sent.message_id,
            thread_id: sent.thread_id,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ApprovalMessageMapFile {
    #[serde(default)]
    messages: HashMap<String, ApprovalMessageRecord>,
}

#[derive(Debug, Clone)]
pub struct ApprovalMessageMap {
    path: PathBuf,
    messages: Arc<RwLock<HashMap<String, ApprovalMessageRecord>>>,
}

impl ApprovalMessageMap {
    pub async fn load_or_default(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let messages = load_messages(&path).await.unwrap_or_default();
        Self {
            path,
            messages: Arc::new(RwLock::new(messages)),
        }
    }

    pub fn ephemeral() -> Self {
        Self {
            path: PathBuf::new(),
            messages: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn record_sent(
        &self,
        request_id: impl Into<String>,
        sent: InteractiveCardSent,
    ) -> anyhow::Result<()> {
        let record = ApprovalMessageRecord::from_sent(request_id, sent);
        let mut messages = self.messages.write().await;
        messages.insert(record.request_id.clone(), record);
        self.persist_locked(&messages).await
    }

    pub async fn get(&self, request_id: &str) -> Option<ApprovalMessageRecord> {
        self.messages.read().await.get(request_id).cloned()
    }

    async fn persist_locked(
        &self,
        messages: &HashMap<String, ApprovalMessageRecord>,
    ) -> anyhow::Result<()> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let payload = serde_json::to_string_pretty(&ApprovalMessageMapFile {
            messages: messages.clone(),
        })?;
        let tmp = self.path.with_extension("tmp");
        tokio::fs::write(&tmp, payload).await?;
        tokio::fs::rename(tmp, &self.path).await?;
        Ok(())
    }
}

async fn load_messages(path: &Path) -> anyhow::Result<HashMap<String, ApprovalMessageRecord>> {
    let raw = match tokio::fs::read_to_string(path).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(err) => return Err(err.into()),
    };
    let parsed: ApprovalMessageMapFile = serde_json::from_str(&raw)?;
    Ok(parsed.messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sent(message_id: &str) -> InteractiveCardSent {
        InteractiveCardSent {
            channel: "slack".to_string(),
            message_id: message_id.to_string(),
            recipient: "C123".to_string(),
            thread_id: Some("1700000000.000100".to_string()),
        }
    }

    #[tokio::test]
    async fn records_and_reads_sent_message() {
        let map = ApprovalMessageMap::ephemeral();
        map.record_sent("req-1", sent("1700000000.000100"))
            .await
            .unwrap();

        let record = map.get("req-1").await.unwrap();
        assert_eq!(record.channel, "slack");
        assert_eq!(record.message_id, "1700000000.000100");
    }

    #[tokio::test]
    async fn persists_message_map_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("approval_message_map.json");
        let map = ApprovalMessageMap::load_or_default(&path).await;
        map.record_sent("req-1", sent("1700000000.000100"))
            .await
            .unwrap();

        let loaded = ApprovalMessageMap::load_or_default(&path).await;
        let record = loaded.get("req-1").await.unwrap();
        assert_eq!(record.recipient, "C123");
        assert_eq!(record.thread_id.as_deref(), Some("1700000000.000100"));
    }
}
