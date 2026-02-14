use anyhow::{anyhow, bail, Result};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(rename = "argsSource", default)]
    pub args_source: Option<String>,
    #[serde(rename = "argsIntegrity", default)]
    pub args_integrity: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct PermissionSnapshot {
    #[serde(default)]
    pub requests: Vec<PermissionRequest>,
    #[serde(default)]
    pub rules: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QuestionChoice {
    pub label: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QuestionInfo {
    #[serde(default)]
    pub header: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<QuestionChoice>,
    #[serde(default)]
    pub multiple: Option<bool>,
    #[serde(default)]
    pub custom: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QuestionToolRef {
    #[serde(rename = "callID", default)]
    pub call_id: Option<String>,
    #[serde(rename = "messageID", default)]
    pub message_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub questions: Vec<QuestionInfo>,
    #[serde(default)]
    pub tool: Option<QuestionToolRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamRequestEvent {
    PermissionAsked(PermissionRequest),
    PermissionReplied { request_id: String, reply: String },
    QuestionAsked(QuestionRequest),
}

#[derive(Debug, Clone)]
pub struct PromptRunResult {
    pub messages: Vec<WireSessionMessage>,
    pub streamed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamEventEnvelope {
    pub event_type: String,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub agent_id: Option<String>,
    pub channel: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct PromptConflictResponse {
    code: Option<String>,
    #[serde(rename = "activeRun")]
    active_run: Option<ActiveRunRef>,
}

#[derive(Debug, Deserialize)]
struct ActiveRunRef {
    #[serde(rename = "runID")]
    run_id: Option<String>,
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
            permission: Some(default_tui_permission_rules()),
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

    pub async fn get_session_messages(&self, session_id: &str) -> Result<Vec<WireSessionMessage>> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let resp = self.client.get(&url).send().await?;
        let messages = resp.json::<Vec<WireSessionMessage>>().await?;
        Ok(messages)
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

    pub async fn list_permissions(&self) -> Result<PermissionSnapshot> {
        let url = format!("{}/permission", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let snapshot = resp.json::<PermissionSnapshot>().await?;
        Ok(snapshot)
    }

    pub async fn reply_permission(&self, id: &str, reply: &str) -> Result<bool> {
        let url = format!("{}/permission/{}/reply", self.base_url, id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "reply": reply }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn list_questions(&self) -> Result<Vec<QuestionRequest>> {
        let url = format!("{}/question", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let snapshot = resp.json::<Vec<QuestionRequest>>().await?;
        Ok(snapshot)
    }

    pub async fn reply_question(&self, id: &str, answers: Vec<Vec<String>>) -> Result<bool> {
        let url = format!("{}/question/{}/reply", self.base_url, id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "answers": answers }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn reject_question(&self, id: &str) -> Result<bool> {
        let url = format!("{}/question/{}/reject", self.base_url, id);
        let resp = self.client.post(&url).send().await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
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
        self.send_prompt_with_stream_events(session_id, message, agent, None, model, |event| {
            if let Some(delta) = extract_delta_text(&event.payload) {
                if !delta.is_empty() {
                    on_delta(delta);
                }
            }
        })
        .await
    }

    pub async fn send_prompt_with_stream_events<F>(
        &self,
        session_id: &str,
        message: &str,
        agent: Option<&str>,
        agent_id: Option<&str>,
        model: Option<ModelSpec>,
        mut on_event: F,
    ) -> Result<PromptRunResult>
    where
        F: FnMut(StreamEventEnvelope),
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
        let mut prompt_req = self
            .client
            .post(&prompt_url)
            .header("Accept", "text/event-stream");
        if let Some(agent_id) = agent_id {
            prompt_req = prompt_req.header("x-tandem-agent-id", agent_id);
        }
        let mut resp = prompt_req.json(&req).send().await?;
        if resp.status() == reqwest::StatusCode::CONFLICT {
            let body = resp.text().await?;
            let run_id = serde_json::from_str::<PromptConflictResponse>(&body)
                .ok()
                .and_then(|payload| {
                    if payload.code.as_deref() == Some("SESSION_RUN_CONFLICT") {
                        payload.active_run.and_then(|run| run.run_id)
                    } else {
                        None
                    }
                });
            if let Some(run_id) = run_id {
                let _ = self.cancel_run_by_id(session_id, &run_id).await;
                tokio::time::sleep(Duration::from_millis(150)).await;
                let mut retry_req = self
                    .client
                    .post(&prompt_url)
                    .header("Accept", "text/event-stream");
                if let Some(agent_id) = agent_id {
                    retry_req = retry_req.header("x-tandem-agent-id", agent_id);
                }
                resp = retry_req.json(&req).send().await?;
            } else {
                bail!("409 Conflict: {}", body);
            }
        }
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
            while let Some(chunk) =
                tokio::time::timeout(Duration::from_secs(90), stream.next()).await?
            {
                let chunk = chunk?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);
                while let Some(payload) = parse_sse_payload(&mut buffer) {
                    if let Some(event) = parse_stream_event_envelope(payload) {
                        if extract_delta_text(&event.payload)
                            .map(|d| !d.is_empty())
                            .unwrap_or(false)
                        {
                            streamed = true;
                        }
                        on_event(event);
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

fn default_tui_permission_rules() -> Vec<serde_json::Value> {
    tandem_core::default_tui_permission_rules()
        .into_iter()
        .map(|rule| {
            serde_json::json!({
                "permission": rule.permission,
                "pattern": rule.pattern,
                "action": rule.action
            })
        })
        .collect()
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

fn parse_stream_event_envelope(payload: serde_json::Value) -> Option<StreamEventEnvelope> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?.to_string();
    let props = payload
        .get("properties")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Some(StreamEventEnvelope {
        event_type,
        session_id: props
            .get("sessionID")
            .or_else(|| props.get("sessionId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        run_id: props
            .get("runID")
            .or_else(|| props.get("run_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        agent_id: props
            .get("agentID")
            .or_else(|| props.get("agent"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        channel: props
            .get("channel")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        payload,
    })
}

pub fn extract_delta_text(payload: &serde_json::Value) -> Option<String> {
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

pub fn extract_stream_activity(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?;

    match event_type {
        "permission.asked" => {
            let tool = props.get("tool").and_then(|v| v.as_str()).unwrap_or("tool");
            let request_id = props
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Some(format!(
                "Waiting for permission: `{}` (request `{}`)",
                tool, request_id
            ))
        }
        "permission.replied" => {
            let request_id = props
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let reply = props
                .get("reply")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Some(format!(
                "Permission `{}` replied with `{}`.",
                request_id, reply
            ))
        }
        "question.asked" => Some("Agent is waiting for your input.".to_string()),
        "message.part.updated" => {
            let Some(part) = props.get("part") else {
                return None;
            };
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if part_type != "tool" {
                return None;
            }
            let tool = part
                .get("tool")
                .or_else(|| part.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            let status = part
                .get("state")
                .and_then(|s| s.get("status"))
                .or_else(|| part.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match status {
                "running" => Some(format!("Running tool `{}`...", tool)),
                "pending" => Some(format!("Tool `{}` pending...", tool)),
                "completed" | "done" => Some(format!("Tool `{}` completed.", tool)),
                "failed" | "error" => Some(format!("Tool `{}` failed.", tool)),
                "cancelled" | "canceled" => Some(format!("Tool `{}` cancelled.", tool)),
                _ => Some(format!("Tool `{}` update.", tool)),
            }
        }
        _ => None,
    }
}

pub fn extract_stream_request(payload: &serde_json::Value) -> Option<StreamRequestEvent> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?.clone();

    match event_type {
        "permission.asked" => {
            let request = serde_json::from_value::<PermissionRequest>(serde_json::json!({
                "id": props
                    .get("requestID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
                "sessionID": props.get("sessionID").cloned().unwrap_or(serde_json::Value::Null),
                "status": "pending",
                "tool": props.get("tool").cloned().unwrap_or(serde_json::Value::Null),
                "args": props.get("args").cloned().unwrap_or(serde_json::Value::Null),
                "argsSource": props
                    .get("argsSource")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "argsIntegrity": props
                    .get("argsIntegrity")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "query": props.get("query").cloned().unwrap_or(serde_json::Value::Null),
            }))
            .ok()?;
            if request.id.trim().is_empty() {
                return None;
            }
            Some(StreamRequestEvent::PermissionAsked(request))
        }
        "permission.replied" => Some(StreamRequestEvent::PermissionReplied {
            request_id: props
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            reply: props
                .get("reply")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
        "question.asked" => {
            let request = serde_json::from_value::<QuestionRequest>(serde_json::json!({
                "id": props.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "sessionID": props.get("sessionID").cloned().unwrap_or(serde_json::Value::Null),
                "questions": props
                    .get("questions")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!([])),
                "tool": props.get("tool").cloned().unwrap_or(serde_json::Value::Null),
            }))
            .ok()?;
            if request.id.trim().is_empty() {
                return None;
            }
            Some(StreamRequestEvent::QuestionAsked(request))
        }
        _ => None,
    }
}

pub fn extract_stream_error(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?;

    if event_type == "session.error" {
        if let Some(message) = props
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|v| v.as_str())
        {
            let code = props
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str())
                .unwrap_or("ENGINE_ERROR");
            return Some(format!("{}: {}", code, message));
        }
        return Some("Engine reported an error.".to_string());
    }

    if event_type == "session.run.finished" {
        let status = props.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status != "completed" {
            let reason = props
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("run did not complete");
            return Some(format!("Run {}: {}", status, reason));
        }
    }

    None
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

    #[test]
    fn parse_stream_event_envelope_extracts_core_fields() {
        let payload = serde_json::json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "s1",
                "runID": "r1",
                "agentID": "A2",
                "channel": "assistant",
                "delta": "hello"
            }
        });
        let envelope = parse_stream_event_envelope(payload.clone()).expect("envelope");
        assert_eq!(envelope.event_type, "message.part.updated");
        assert_eq!(envelope.session_id.as_deref(), Some("s1"));
        assert_eq!(envelope.run_id.as_deref(), Some("r1"));
        assert_eq!(envelope.agent_id.as_deref(), Some("A2"));
        assert_eq!(envelope.channel.as_deref(), Some("assistant"));
        assert_eq!(envelope.payload, payload);
    }

    #[test]
    fn parse_sse_payload_reads_data_block() {
        let mut buffer =
            "event: message\ndata: {\"type\":\"message.part.updated\",\"properties\":{\"delta\":\"x\"}}\n\n"
                .to_string();
        let parsed = parse_sse_payload(&mut buffer).expect("payload");
        assert_eq!(
            parsed.get("type").and_then(|v| v.as_str()),
            Some("message.part.updated")
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn extract_stream_error_reads_session_error() {
        let payload = serde_json::json!({
            "type": "session.error",
            "properties": {
                "error": { "code": "PROVIDER_AUTH", "message": "missing API key" }
            }
        });
        let msg = extract_stream_error(&payload).expect("error");
        assert!(msg.contains("PROVIDER_AUTH"));
        assert!(msg.contains("missing API key"));
    }
}
