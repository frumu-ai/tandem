// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#![cfg_attr(
    not(test),
    deny(clippy::expect_used, clippy::panic, clippy::unwrap_used)
)]

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalCallbackRecord {
    pub callback_id: String,
    pub request_id: String,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub channel: String,
    pub recipient: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ApprovalMessageMapFile {
    #[serde(default)]
    messages: HashMap<String, ApprovalMessageRecord>,
    #[serde(default)]
    run_threads: HashMap<String, ApprovalMessageRecord>,
    #[serde(default)]
    telegram_callbacks: HashMap<String, ApprovalCallbackRecord>,
}

#[derive(Debug, Clone)]
pub struct ApprovalMessageMap {
    path: PathBuf,
    data: Arc<RwLock<ApprovalMessageMapFile>>,
}

impl ApprovalMessageMap {
    pub async fn load_or_default(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let data = load_message_map(&path).await.unwrap_or_default();
        Self {
            path,
            data: Arc::new(RwLock::new(data)),
        }
    }

    pub fn ephemeral() -> Self {
        Self {
            path: PathBuf::new(),
            data: Arc::new(RwLock::new(ApprovalMessageMapFile::default())),
        }
    }

    pub async fn record_sent(
        &self,
        request_id: impl Into<String>,
        sent: InteractiveCardSent,
    ) -> anyhow::Result<()> {
        let record = ApprovalMessageRecord::from_sent(request_id, sent);
        self.record_message(record, None).await
    }

    pub async fn record_approval_sent(
        &self,
        request: &tandem_types::ApprovalRequest,
        sent: InteractiveCardSent,
    ) -> anyhow::Result<()> {
        let record = ApprovalMessageRecord::from_sent(request.request_id.clone(), sent);
        self.record_message(record, Some(request.run_id.as_str()))
            .await
    }

    pub async fn record_telegram_callback(
        &self,
        callback_id: impl Into<String>,
        request: &tandem_types::ApprovalRequest,
        recipient: impl Into<String>,
    ) -> anyhow::Result<()> {
        let callback_id = callback_id.into();
        let record = ApprovalCallbackRecord {
            callback_id: callback_id.clone(),
            request_id: request.request_id.clone(),
            run_id: request.run_id.clone(),
            node_id: request.node_id.clone(),
            channel: "telegram".to_string(),
            recipient: recipient.into(),
        };
        let mut data = self.data.write().await;
        data.telegram_callbacks.insert(callback_id, record);
        self.persist_locked(&data).await
    }

    async fn record_message(
        &self,
        record: ApprovalMessageRecord,
        run_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut data = self.data.write().await;
        if let Some(run_id) = run_id.map(str::trim).filter(|value| !value.is_empty()) {
            data.run_threads.insert(run_id.to_string(), record.clone());
        }
        data.messages.insert(record.request_id.clone(), record);
        self.persist_locked(&data).await
    }

    pub async fn get(&self, request_id: &str) -> Option<ApprovalMessageRecord> {
        self.data.read().await.messages.get(request_id).cloned()
    }

    pub async fn get_thread_for_run(&self, run_id: &str) -> Option<ApprovalMessageRecord> {
        self.data.read().await.run_threads.get(run_id).cloned()
    }

    pub async fn get_telegram_callback(&self, callback_id: &str) -> Option<ApprovalCallbackRecord> {
        self.data
            .read()
            .await
            .telegram_callbacks
            .get(callback_id)
            .cloned()
    }

    async fn persist_locked(&self, data: &ApprovalMessageMapFile) -> anyhow::Result<()> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let payload = serde_json::to_string_pretty(data)?;
        let tmp = self.path.with_extension("tmp");
        tokio::fs::write(&tmp, payload).await?;
        tokio::fs::rename(tmp, &self.path).await?;
        Ok(())
    }
}

async fn load_message_map(path: &Path) -> anyhow::Result<ApprovalMessageMapFile> {
    let raw = match tokio::fs::read_to_string(path).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ApprovalMessageMapFile::default())
        }
        Err(err) => return Err(err.into()),
    };
    serde_json::from_str(&raw).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_types::{ApprovalDecision, ApprovalRequest, ApprovalSourceKind, ApprovalTenantRef};

    fn sent(message_id: &str) -> InteractiveCardSent {
        InteractiveCardSent {
            channel: "slack".to_string(),
            message_id: message_id.to_string(),
            recipient: "C123".to_string(),
            thread_id: Some("1700000000.000100".to_string()),
        }
    }

    fn request(run_id: &str) -> ApprovalRequest {
        ApprovalRequest {
            request_id: format!("automation_v2:{run_id}:send_email"),
            approval_wait: None,
            source: ApprovalSourceKind::AutomationV2,
            tenant: ApprovalTenantRef {
                org_id: "org".to_string(),
                workspace_id: "workspace".to_string(),
                user_id: None,
            },
            run_id: run_id.to_string(),
            node_id: Some("send_email".to_string()),
            workflow_name: Some("Sales outreach".to_string()),
            action_kind: Some("send email".to_string()),
            action_preview_markdown: None,
            surface_payload: None,
            requested_at_ms: 1,
            expires_at_ms: None,
            decisions: vec![ApprovalDecision::Approve],
            rework_targets: vec![],
            instructions: None,
            decided_by: None,
            decided_at_ms: None,
            decision: None,
            rework_feedback: None,
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

    #[tokio::test]
    async fn records_run_thread_lookup() {
        let map = ApprovalMessageMap::ephemeral();
        let request = request("run-1");
        map.record_approval_sent(&request, sent("1700000000.000100"))
            .await
            .unwrap();

        let record = map.get_thread_for_run("run-1").await.unwrap();
        assert_eq!(record.request_id, "automation_v2:run-1:send_email");
        assert_eq!(record.thread_id.as_deref(), Some("1700000000.000100"));
    }

    #[tokio::test]
    async fn persists_run_thread_lookup_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("approval_message_map.json");
        let map = ApprovalMessageMap::load_or_default(&path).await;
        let request = request("run-1");
        map.record_approval_sent(&request, sent("1700000000.000100"))
            .await
            .unwrap();

        let loaded = ApprovalMessageMap::load_or_default(&path).await;
        let record = loaded.get_thread_for_run("run-1").await.unwrap();
        assert_eq!(record.recipient, "C123");
        assert_eq!(record.message_id, "1700000000.000100");
    }

    #[tokio::test]
    async fn records_and_reads_telegram_callback_mapping() {
        let map = ApprovalMessageMap::ephemeral();
        let request = request("run-abcdef");
        map.record_telegram_callback("tgcb_123", &request, "12345")
            .await
            .unwrap();

        let record = map.get_telegram_callback("tgcb_123").await.unwrap();
        assert_eq!(record.request_id, "automation_v2:run-abcdef:send_email");
        assert_eq!(record.run_id, "run-abcdef");
        assert_eq!(record.node_id.as_deref(), Some("send_email"));
        assert_eq!(record.recipient, "12345");
    }

    #[tokio::test]
    async fn persists_telegram_callback_mapping_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("approval_message_map.json");
        let map = ApprovalMessageMap::load_or_default(&path).await;
        let request = request("run-abcdef");
        map.record_telegram_callback("tgcb_123", &request, "12345")
            .await
            .unwrap();

        let loaded = ApprovalMessageMap::load_or_default(&path).await;
        let record = loaded.get_telegram_callback("tgcb_123").await.unwrap();
        assert_eq!(record.run_id, "run-abcdef");
        assert_eq!(record.node_id.as_deref(), Some("send_email"));
    }
}
