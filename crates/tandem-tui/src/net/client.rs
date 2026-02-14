use anyhow::{anyhow, bail, Result};
use futures::StreamExt;
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

#[derive(Debug, Clone)]
pub struct PromptRunResult {
    pub messages: Vec<WireSessionMessage>,
    pub streamed: bool,
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
        let result = self
            .send_prompt_with_stream(session_id, message, agent, model, |_| {})
            .await?;
        Ok(result.messages)
    }

    pub async fn send_prompt_with_stream<F>(
        &self,
        session_id: &str,
        message: &str,
        agent: Option<&str>,
        model: Option<ModelSpec>,
        mut on_delta: F,
    ) -> Result<PromptRunResult>
    where
        F: FnMut(String),
    {
        let append_url = format!(
            "{}/session/{}/message?mode=append",
            self.base_url, session_id
        );
        let prompt_url = format!("{}/session/{}/prompt_sync", self.base_url, session_id);
        let req = SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: message.to_string(),
            }],
            model,
            agent: agent.map(String::from),
        };
        let append_resp = self.client.post(&append_url).json(&req).send().await?;
        if !append_resp.status().is_success() {
            let status = append_resp.status();
            let body = append_resp.text().await?;
            bail!("append failed {}: {}", status, body);
        }
        let resp = self
            .client
            .post(&prompt_url)
            .header("Accept", "text/event-stream")
            .json(&req)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await?;
            bail!("{}: {}", status, body);
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if content_type.starts_with("text/event-stream") {
            let mut stream = resp.bytes_stream();
            let mut streamed = false;
            let mut buffer = String::new();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);
                while let Some(payload) = parse_sse_payload(&mut buffer) {
                    if let Some(delta) = extract_delta_text(&payload) {
                        if !delta.is_empty() {
                            streamed = true;
                            on_delta(delta);
                        }
                    }
                }
            }
            let final_url = format!("{}/session/{}/message", self.base_url, session_id);
            let final_resp = self.client.get(&final_url).send().await?;
            let final_status = final_resp.status();
            let final_body = final_resp.text().await?;
            if !final_status.is_success() {
                bail!("{}: {}", final_status, final_body);
            }
            let messages: Vec<WireSessionMessage> = serde_json::from_str(&final_body)
                .map_err(|err| anyhow!("Invalid response body: {} | body: {}", err, final_body))?;
            return Ok(PromptRunResult { messages, streamed });
        }
        let body = resp.text().await?;
        let messages: Vec<WireSessionMessage> = serde_json::from_str(&body)
            .map_err(|err| anyhow!("Invalid response body: {} | body: {}", err, body))?;
        Ok(PromptRunResult {
            messages,
            streamed: false,
        })
    }

    pub async fn abort_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/session/{}/cancel", self.base_url, session_id);
        self.client.post(&url).send().await?;
        Ok(())
    }

    pub async fn cancel_run_by_id(&self, session_id: &str, run_id: &str) -> Result<bool> {
        let url = format!(
            "{}/session/{}/run/{}/cancel",
            self.base_url, session_id, run_id
        );
        let resp = self.client.post(&url).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
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

fn parse_sse_payload(buffer: &mut String) -> Option<serde_json::Value> {
    let (end_idx, delim_len) = if let Some(i) = buffer.find("\r\n\r\n") {
        (i, 4)
    } else if let Some(i) = buffer.find("\n\n") {
        (i, 2)
    } else {
        return None;
    };

    let event_str = buffer[..end_idx].to_string();
    *buffer = buffer[end_idx + delim_len..].to_string();

    let mut data_lines: Vec<String> = Vec::new();
    for raw_line in event_str.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let data = data_lines.join("\n");
    if data == "[DONE]" {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&data).ok()
}

fn extract_delta_text(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    if event_type != "message.part.updated" {
        return None;
    }
    let props = payload.get("properties")?;
    if let Some(delta) = props.get("delta") {
        return match delta {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(map) => map
                .get("text")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            serde_json::Value::Array(items) => {
                let text = items
                    .iter()
                    .filter_map(|item| match item {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(map) => map
                            .get("text")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            _ => None,
        };
    }
    // Some runtime snapshots only include the final text payload without explicit delta.
    props
        .get("part")
        .and_then(|p| p.get("text"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_single_response_server(
        expected_path: &'static str,
        response_status: &'static str,
        response_body: &'static str,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await.expect("read");
            let req = String::from_utf8_lossy(&buf[..n]);
            let first_line = req.lines().next().unwrap_or("");
            assert!(
                first_line.contains(expected_path),
                "expected path {}, got {}",
                expected_path,
                first_line
            );
            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_status,
                response_body.len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write_all");
        });
        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn cancel_run_by_id_posts_expected_endpoint() {
        let base = spawn_single_response_server(
            "/session/s1/run/run_42/cancel",
            "200 OK",
            r#"{"ok":true,"cancelled":true}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let cancelled = client
            .cancel_run_by_id("s1", "run_42")
            .await
            .expect("cancel");
        assert!(cancelled);
    }

    #[tokio::test]
    async fn cancel_run_by_id_returns_false_for_non_active_run() {
        let base = spawn_single_response_server(
            "/session/s1/run/run_missing/cancel",
            "200 OK",
            r#"{"ok":true,"cancelled":false}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let cancelled = client
            .cancel_run_by_id("s1", "run_missing")
            .await
            .expect("cancel");
        assert!(!cancelled);
    }
}
