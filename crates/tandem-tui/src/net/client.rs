use anyhow::{anyhow, bail, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tandem_types::{CreateSessionRequest, ModelSpec};
use tandem_wire::{WireProviderEntry, WireSessionMessage};

#[derive(Clone)]
pub struct EngineClient {
    base_url: String,
    client: Client,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EngineStatus {
    pub healthy: bool,
    pub version: String,
    pub mode: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct SessionTime {
    pub created: Option<u64>,
    pub updated: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(rename = "workspaceRoot", default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub time: Option<SessionTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionScope {
    Workspace,
    Global,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ProviderCatalog {
    pub all: Vec<WireProviderEntry>,
    pub connected: Vec<String>,
    pub default: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct ConfigProvidersResponse {
    pub providers: HashMap<String, ProviderConfigEntry>,
    pub default: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct ProviderConfigEntry {
    pub api_key: Option<String>,
    pub url: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EngineLease {
    pub lease_id: String,
    pub client_id: String,
    pub client_type: String,
    pub acquired_at_ms: u64,
    pub last_renewed_at_ms: u64,
    pub ttl_ms: u64,
    pub lease_count: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SendMessageRequest {
    #[serde(default)]
    pub parts: Vec<MessagePartInput>,
    pub model: Option<ModelSpec>,
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagePartInput {
    Text {
        text: String,
    },
    File {
        mime: String,
        filename: Option<String>,
        url: String,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
    pub model: Option<ModelSpec>,
    pub provider: Option<String>,
    pub mode: Option<String>,
}

impl EngineClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
            api_key: None,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn check_health(&self) -> Result<bool> {
        let url = format!("{}/global/health", self.base_url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp.status().is_success())
    }

    pub async fn get_engine_status(&self) -> Result<EngineStatus> {
        let url = format!("{}/global/health", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let status = resp.json::<EngineStatus>().await?;
        Ok(status)
    }

    pub async fn acquire_lease(
        &self,
        client_id: &str,
        client_type: &str,
        ttl_ms: Option<u64>,
    ) -> Result<EngineLease> {
        let url = format!("{}/global/lease/acquire", self.base_url);
        let payload = serde_json::json!({
            "client_id": client_id,
            "client_type": client_type,
            "ttl_ms": ttl_ms.unwrap_or(60_000),
        });
        let resp = self.client.post(&url).json(&payload).send().await?;
        let lease = resp.json::<EngineLease>().await?;
        Ok(lease)
    }

    pub async fn renew_lease(&self, lease_id: &str) -> Result<bool> {
        let url = format!("{}/global/lease/renew", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "lease_id": lease_id }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn release_lease(&self, lease_id: &str) -> Result<bool> {
        let url = format!("{}/global/lease/release", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "lease_id": lease_id }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let workspace = std::env::current_dir()
            .ok()
            .and_then(|p| normalize_workspace_path(&p));
        self.list_sessions_scoped(SessionScope::Workspace, workspace)
            .await
    }

    pub async fn list_sessions_scoped(
        &self,
        scope: SessionScope,
        workspace: Option<String>,
    ) -> Result<Vec<Session>> {
        let url = format!("{}/api/session", self.base_url);
        let scope_value = match scope {
            SessionScope::Workspace => "workspace",
            SessionScope::Global => "global",
        };
        let mut req = self.client.get(&url).query(&[("scope", scope_value)]);
        if matches!(scope, SessionScope::Workspace) {
            if let Some(workspace) = workspace {
                req = req.query(&[("workspace", workspace)]);
            }
        }
        let resp = req.send().await?;
        let sessions = resp.json::<Vec<Session>>().await?;
        Ok(sessions)
    }

    pub async fn create_session(&self, title: Option<String>) -> Result<Session> {
        let url = format!("{}/api/session", self.base_url);
        let req = CreateSessionRequest {
            parent_id: None,
            title,
            directory: std::env::current_dir()
                .ok()
                .and_then(|p| normalize_workspace_path(&p)),
            workspace_root: std::env::current_dir()
                .ok()
                .and_then(|p| normalize_workspace_path(&p)),
            model: None,
            provider: None,
            permission: None,
        };

        let resp = self.client.post(&url).json(&req).send().await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Session> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        let resp = self.client.get(&url).send().await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn update_session(
        &self,
        session_id: &str,
        req: UpdateSessionRequest,
    ) -> Result<Session> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        let resp = self.client.patch(&url).json(&req).send().await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn list_providers(&self) -> Result<ProviderCatalog> {
        let url = format!("{}/provider", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let catalog = resp.json::<ProviderCatalog>().await?;
        Ok(catalog)
    }

    pub async fn config_providers(&self) -> Result<ConfigProvidersResponse> {
        let url = format!("{}/config/providers", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let config = resp.json::<ConfigProvidersResponse>().await?;
        Ok(config)
    }

    pub async fn set_auth(&self, provider_id: &str, api_key: &str) -> Result<()> {
        let url = format!("{}/auth/{}", self.base_url, provider_id);
        self.client
            .put(&url)
            .json(&serde_json::json!({ "apiKey": api_key }))
            .send()
            .await?;
        Ok(())
    }

    pub async fn delete_auth(&self, provider_id: &str) -> Result<()> {
        let url = format!("{}/auth/{}", self.base_url, provider_id);
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn send_prompt(
        &self,
        session_id: &str,
        message: &str,
        agent: Option<&str>,
        model: Option<ModelSpec>,
    ) -> Result<Vec<WireSessionMessage>> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let req = SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: message.to_string(),
            }],
            model,
            agent: agent.map(String::from),
        };
        let resp = self.client.post(&url).json(&req).send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            bail!("{}: {}", status, body);
        }
        let messages: Vec<WireSessionMessage> = serde_json::from_str(&body)
            .map_err(|err| anyhow!("Invalid response body: {} | body: {}", err, body))?;
        Ok(messages)
    }

    pub async fn abort_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/session/{}/abort", self.base_url, session_id);
        self.client.post(&url).send().await?;
        Ok(())
    }

    pub async fn get_config(&self) -> Result<serde_json::Value> {
        let url = format!("{}/config", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let config = resp.json::<serde_json::Value>().await?;
        Ok(config)
    }

    pub async fn patch_config(&self, patch: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/config", self.base_url);
        let resp = self.client.patch(&url).json(&patch).send().await?;
        let config = resp.json::<serde_json::Value>().await?;
        Ok(config)
    }

    pub async fn attach_session_to_workspace(
        &self,
        session_id: &str,
        target_workspace: &str,
        reason_tag: &str,
    ) -> Result<Session> {
        let url = format!("{}/api/session/{}/attach", self.base_url, session_id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "target_workspace": target_workspace,
                "reason_tag": reason_tag
            }))
            .send()
            .await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }
}

fn normalize_workspace_path(path: &PathBuf) -> Option<String> {
    let absolute = if path.is_absolute() {
        path.clone()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let normalized = if absolute.exists() {
        absolute.canonicalize().ok()?
    } else {
        absolute
    };
    Some(normalized.to_string_lossy().to_string())
}
