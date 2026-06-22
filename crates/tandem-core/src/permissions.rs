use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{watch, Mutex, RwLock};
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tandem_types::EngineEvent;

use crate::event_bus::EventBus;

const PERMISSION_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionAction {
    Allow,
    Ask,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub id: String,
    pub permission: String,
    pub pattern: String,
    pub action: PermissionAction,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "createdAtMs"
    )]
    pub created_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "createdBy")]
    pub created_by: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "sourceRequestID"
    )]
    pub source_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sessionID")]
    pub session_id: Option<String>,
    pub permission: String,
    pub pattern: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "argsSource")]
    pub args_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "argsIntegrity")]
    pub args_integrity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    pub status: String,
    #[serde(default, rename = "requestedAtMs")]
    pub requested_at_ms: u64,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "decidedAtMs"
    )]
    pub decided_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "decidedBy")]
    pub decided_by: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "decisionReason"
    )]
    pub decision_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionDecisionRecord {
    #[serde(rename = "requestID")]
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none", rename = "sessionID")]
    pub session_id: Option<String>,
    pub permission: String,
    pub pattern: String,
    pub decision: String,
    #[serde(rename = "decidedAtMs")]
    pub decided_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none", rename = "decidedBy")]
    pub decided_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "standingRuleID")]
    pub standing_rule_id: Option<String>,
    #[serde(rename = "standingRulePersisted")]
    pub standing_rule_persisted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionReplyOutcome {
    pub request: PermissionRequest,
    pub decision: PermissionDecisionRecord,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<PermissionRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PermissionStateFile {
    schema_version: u32,
    requests: HashMap<String, PermissionRequest>,
    rules: Vec<PermissionRule>,
    decisions: Vec<PermissionDecisionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionArgsContext {
    #[serde(rename = "argsSource")]
    pub args_source: String,
    #[serde(rename = "argsIntegrity")]
    pub args_integrity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
}

/// Returns true when persisting a standing "always" allow rule for the given
/// permission/pattern would be too broad to be safe. Shell/execution tools are
/// excluded from standing approvals because a blanket allow would auto-approve
/// arbitrary future commands.
fn standing_allow_is_unsafe(permission: &str, pattern: &str) -> bool {
    use crate::tool_capabilities::{
        canonical_tool_name, tool_name_matches_profile, ToolCapabilityProfile,
    };
    [permission, pattern].into_iter().any(|name| {
        // Profile matching only canonicalizes execution tools to `bash`, so also
        // match execution capability names that are keyed directly (for example
        // the automation `verify_command` capability) to close the standing-allow
        // path for arbitrary command execution/verification.
        tool_name_matches_profile(name, ToolCapabilityProfile::ShellExecution)
            || tool_name_matches_profile(name, ToolCapabilityProfile::VerifyCommand)
            || matches!(
                canonical_tool_name(name).as_str(),
                "verify_command"
                    | "verifycommand"
                    | "shell"
                    | "exec"
                    | "execute"
                    | "command"
                    | "run"
                    | "run_command"
                    | "runcommand"
                    | "terminal"
            )
    })
}

#[derive(Clone)]
pub struct PermissionManager {
    requests: Arc<RwLock<HashMap<String, PermissionRequest>>>,
    rules: Arc<RwLock<Vec<PermissionRule>>>,
    decisions: Arc<RwLock<Vec<PermissionDecisionRecord>>>,
    waiters: Arc<RwLock<HashMap<String, watch::Sender<Option<String>>>>>,
    state_path: Arc<RwLock<Option<PathBuf>>>,
    state_write_lock: Arc<Mutex<()>>,
    event_bus: EventBus,
}

impl PermissionManager {
    pub fn new(event_bus: EventBus) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            rules: Arc::new(RwLock::new(Vec::new())),
            decisions: Arc::new(RwLock::new(Vec::new())),
            waiters: Arc::new(RwLock::new(HashMap::new())),
            state_path: Arc::new(RwLock::new(None)),
            state_write_lock: Arc::new(Mutex::new(())),
            event_bus,
        }
    }

    pub async fn new_with_state_file(
        event_bus: EventBus,
        path: impl Into<PathBuf>,
    ) -> anyhow::Result<Self> {
        let manager = Self::new(event_bus);
        manager.load_state_file(path).await?;
        Ok(manager)
    }

    pub async fn load_state_file(&self, path: impl Into<PathBuf>) -> anyhow::Result<usize> {
        let path = path.into();
        *self.state_path.write().await = Some(path.clone());

        let raw = match tokio::fs::read_to_string(&path).await {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.persist_state().await?;
                return Ok(0);
            }
            Err(error) => return Err(error).context("failed to read permission state file"),
        };
        if raw.trim().is_empty() {
            self.persist_state().await?;
            return Ok(0);
        }
        let mut file: PermissionStateFile =
            serde_json::from_str(&raw).context("failed to parse permission state file")?;
        if file.schema_version > PERMISSION_STATE_SCHEMA_VERSION {
            anyhow::bail!(
                "permission state schema_version {} is newer than supported version {}",
                file.schema_version,
                PERMISSION_STATE_SCHEMA_VERSION
            );
        }

        let mut restarted_pending = 0usize;
        let now = now_ms();
        for request in file.requests.values_mut() {
            if request.requested_at_ms == 0 {
                request.requested_at_ms = now;
            }
            if request.status == "pending" {
                request.status = "runtime_restarted".to_string();
                request.decided_at_ms = Some(now);
                request.decision_reason =
                    Some("runtime restarted before the permission request was decided".to_string());
                file.decisions.push(PermissionDecisionRecord {
                    request_id: request.id.clone(),
                    session_id: request.session_id.clone(),
                    permission: request.permission.clone(),
                    pattern: request.pattern.clone(),
                    decision: "runtime_restarted".to_string(),
                    decided_at_ms: now,
                    decided_by: Some("system".to_string()),
                    reason: request.decision_reason.clone(),
                    standing_rule_id: None,
                    standing_rule_persisted: false,
                });
                restarted_pending = restarted_pending.saturating_add(1);
            }
        }

        *self.requests.write().await = file.requests;
        *self.rules.write().await = file.rules;
        *self.decisions.write().await = file.decisions;
        self.persist_state().await?;
        Ok(restarted_pending)
    }

    pub async fn evaluate(&self, permission: &str, pattern: &str) -> PermissionAction {
        let permission = normalize_permission_alias(permission);
        let pattern = normalize_permission_alias(pattern);
        let rules = self.rules.read().await;
        let matches_rule = |rule: &&PermissionRule| {
            wildcard_matches(&normalize_permission_alias(&rule.permission), &permission)
                && wildcard_matches(&normalize_permission_alias(&rule.pattern), &pattern)
        };
        if rules
            .iter()
            .filter(matches_rule)
            .any(|rule| matches!(rule.action, PermissionAction::Deny))
        {
            return PermissionAction::Deny;
        }
        if let Some(rule) = rules.iter().rev().find(matches_rule) {
            return rule.action.clone();
        }
        PermissionAction::Ask
    }

    /// Convenience wrapper for the common case where both the permission name
    /// and the match pattern are the same tool name. Prefer this over
    /// `evaluate(&tool, &tool)` at call sites to make the intent explicit.
    pub async fn evaluate_tool(&self, tool_name: &str) -> PermissionAction {
        self.evaluate(tool_name, tool_name).await
    }

    pub async fn ask_for_session(
        &self,
        session_id: Option<&str>,
        tool: &str,
        args: Value,
    ) -> PermissionRequest {
        self.ask_for_session_with_context(session_id, tool, args, None)
            .await
    }

    pub async fn ask_for_session_with_context(
        &self,
        session_id: Option<&str>,
        tool: &str,
        args: Value,
        context: Option<PermissionArgsContext>,
    ) -> PermissionRequest {
        let req = PermissionRequest {
            id: Uuid::new_v4().to_string(),
            session_id: session_id.map(ToString::to_string),
            permission: tool.to_string(),
            pattern: tool.to_string(),
            tool: Some(tool.to_string()),
            args: Some(args.clone()),
            args_source: context.as_ref().map(|c| c.args_source.clone()),
            args_integrity: context.as_ref().map(|c| c.args_integrity.clone()),
            query: context.as_ref().and_then(|c| c.query.clone()),
            status: "pending".to_string(),
            requested_at_ms: now_ms(),
            decided_at_ms: None,
            decided_by: None,
            decision_reason: None,
        };
        let (tx, _rx) = watch::channel(None);
        self.requests
            .write()
            .await
            .insert(req.id.clone(), req.clone());
        self.waiters.write().await.insert(req.id.clone(), tx);
        if let Err(error) = self.persist_state().await {
            tracing::warn!(?error, "failed to persist permission request");
        }
        self.event_bus.publish(EngineEvent::new(
            "permission.asked",
            json!({
                "sessionID": session_id.unwrap_or_default(),
                "requestID": req.id,
                "tool": tool,
                "args": args,
                "argsSource": req.args_source,
                "argsIntegrity": req.args_integrity,
                "query": req.query,
                "requestedAtMs": req.requested_at_ms
            }),
        ));
        req
    }

    pub async fn ask(&self, permission: &str, pattern: &str) -> PermissionRequest {
        let tool = if permission.is_empty() {
            pattern.to_string()
        } else {
            permission.to_string()
        };
        self.ask_for_session(None, &tool, json!({})).await
    }

    pub async fn list(&self) -> Vec<PermissionRequest> {
        self.requests.read().await.values().cloned().collect()
    }

    pub async fn list_rules(&self) -> Vec<PermissionRule> {
        self.rules.read().await.clone()
    }

    pub async fn list_decisions(&self) -> Vec<PermissionDecisionRecord> {
        self.decisions.read().await.clone()
    }

    async fn persist_state(&self) -> anyhow::Result<()> {
        let _write_guard = self.state_write_lock.lock().await;
        let Some(path) = self.state_path.read().await.clone() else {
            return Ok(());
        };
        let file = PermissionStateFile {
            schema_version: PERMISSION_STATE_SCHEMA_VERSION,
            requests: self.requests.read().await.clone(),
            rules: self.rules.read().await.clone(),
            decisions: self.decisions.read().await.clone(),
        };
        write_permission_state_file(&path, &file).await
    }

    pub async fn add_rule(
        &self,
        permission: impl Into<String>,
        pattern: impl Into<String>,
        action: PermissionAction,
    ) -> PermissionRule {
        let rule = PermissionRule {
            id: Uuid::new_v4().to_string(),
            permission: permission.into(),
            pattern: pattern.into(),
            action,
            created_at_ms: Some(now_ms()),
            created_by: Some("system".to_string()),
            source_request_id: None,
            provenance: Some("default_or_system_rule".to_string()),
        };
        let mut rules = self.rules.write().await;
        if rules.iter().any(|existing| {
            existing.permission == rule.permission
                && existing.pattern == rule.pattern
                && std::mem::discriminant(&existing.action) == std::mem::discriminant(&rule.action)
        }) {
            return rule;
        }
        rules.push(rule.clone());
        drop(rules);
        if let Err(error) = self.persist_state().await {
            tracing::warn!(?error, "failed to persist permission rule");
        }
        rule
    }

    pub async fn reply(&self, id: &str, reply: &str) -> bool {
        self.reply_with_provenance(id, reply, None, None)
            .await
            .is_some()
    }

    pub async fn reply_with_provenance(
        &self,
        id: &str,
        reply: &str,
        decided_by: Option<String>,
        reason: Option<String>,
    ) -> Option<PermissionReplyOutcome> {
        let now = now_ms();
        let request = {
            let mut requests = self.requests.write().await;
            let Some(req) = requests.get_mut(id) else {
                return None;
            };
            if req.status != "pending" {
                return None;
            }
            req.status = reply.to_string();
            req.decided_at_ms = Some(now);
            req.decided_by = decided_by.clone();
            req.decision_reason = reason.clone();
            req.clone()
        };

        let mut rule = None;
        if matches!(reply, "always" | "allow") {
            // SEC-03: never create an overly broad *standing* approval for
            // shell/execution tools. A blanket `bash` allow would auto-approve
            // arbitrary future commands, so for these high-risk tools "always"
            // is treated as a one-time approval (the current request is still
            // approved by the waiter; no persistent Allow rule is recorded).
            if !standing_allow_is_unsafe(&request.permission, &request.pattern) {
                let standing_rule = PermissionRule {
                    id: Uuid::new_v4().to_string(),
                    permission: request.permission.clone(),
                    pattern: request.pattern.clone(),
                    action: PermissionAction::Allow,
                    created_at_ms: Some(now),
                    created_by: decided_by.clone().or_else(|| Some("unknown".to_string())),
                    source_request_id: Some(request.id.clone()),
                    provenance: Some("permission_reply".to_string()),
                };
                self.rules.write().await.push(standing_rule.clone());
                rule = Some(standing_rule);
            }
        } else if matches!(reply, "reject" | "deny") {
            let standing_rule = PermissionRule {
                id: Uuid::new_v4().to_string(),
                permission: request.permission.clone(),
                pattern: request.pattern.clone(),
                action: PermissionAction::Deny,
                created_at_ms: Some(now),
                created_by: decided_by.clone().or_else(|| Some("unknown".to_string())),
                source_request_id: Some(request.id.clone()),
                provenance: Some("permission_reply".to_string()),
            };
            self.rules.write().await.push(standing_rule.clone());
            rule = Some(standing_rule);
        }

        let decision = PermissionDecisionRecord {
            request_id: request.id.clone(),
            session_id: request.session_id.clone(),
            permission: request.permission.clone(),
            pattern: request.pattern.clone(),
            decision: reply.to_string(),
            decided_at_ms: now,
            decided_by: decided_by.clone(),
            reason,
            standing_rule_id: rule.as_ref().map(|rule| rule.id.clone()),
            standing_rule_persisted: rule.is_some(),
        };
        self.decisions.write().await.push(decision.clone());
        if let Err(error) = self.persist_state().await {
            tracing::warn!(?error, "failed to persist permission reply");
        }
        self.event_bus.publish(EngineEvent::new(
            "permission.replied",
            json!({
                "requestID": id,
                "reply": reply,
                "decidedAtMs": now,
                "decidedBy": decided_by,
                "standingRuleID": rule.as_ref().map(|rule| rule.id.clone()),
                "standingRulePersisted": rule.is_some()
            }),
        ));
        if let Some(waiter) = self.waiters.read().await.get(id).cloned() {
            let _ = waiter.send(Some(reply.to_string()));
        }
        Some(PermissionReplyOutcome {
            request,
            decision,
            rule,
        })
    }

    pub async fn wait_for_reply(&self, id: &str, cancel: CancellationToken) -> Option<String> {
        let (reply, _timed_out) = self.wait_for_reply_with_timeout(id, cancel, None).await;
        reply
    }

    pub async fn wait_for_reply_with_timeout(
        &self,
        id: &str,
        cancel: CancellationToken,
        timeout: Option<Duration>,
    ) -> (Option<String>, bool) {
        let mut rx = {
            let waiters = self.waiters.read().await;
            let Some(tx) = waiters.get(id) else {
                return (None, false);
            };
            tx.subscribe()
        };
        let immediate = { rx.borrow().clone() };
        if let Some(reply) = immediate {
            self.waiters.write().await.remove(id);
            return (Some(reply), false);
        }

        let (waited, timed_out): (Option<String>, bool) = match timeout {
            Some(duration) => {
                let timeout_sleep = tokio::time::sleep(duration);
                tokio::pin!(timeout_sleep);
                tokio::select! {
                    _ = cancel.cancelled() => (None, false),
                    _ = &mut timeout_sleep => (None, true),
                    changed = rx.changed() => {
                        if changed.is_ok() {
                            let updated = { rx.borrow().clone() };
                            (updated, false)
                        } else {
                            (None, false)
                        }
                    }
                }
            }
            None => {
                let waited = tokio::select! {
                    _ = cancel.cancelled() => None,
                    changed = rx.changed() => {
                        if changed.is_ok() {
                            let updated = { rx.borrow().clone() };
                            updated
                        } else {
                            None
                        }
                    }
                };
                (waited, false)
            }
        };
        self.waiters.write().await.remove(id);
        (waited, timed_out)
    }
}

async fn write_permission_state_file(
    path: &Path,
    file: &PermissionStateFile,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .context("failed to create permission state directory")?;
    }
    let payload =
        serde_json::to_string_pretty(file).context("failed to serialize permission state file")?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("permissions");
    let tmp = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    tokio::fs::write(&tmp, payload)
        .await
        .context("failed to write temporary permission state file")?;
    match tokio::fs::rename(&tmp, path).await {
        Ok(()) => Ok(()),
        Err(rename_error) => {
            let _ = tokio::fs::remove_file(path).await;
            tokio::fs::rename(&tmp, path).await.with_context(|| {
                format!("failed to replace permission state file after {rename_error}")
            })
        }
    }
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == value;
    }
    let mut remaining = value;
    let mut is_first = true;
    for part in pattern.split('*') {
        if part.is_empty() {
            continue;
        }
        if is_first {
            if let Some(stripped) = remaining.strip_prefix(part) {
                remaining = stripped;
            } else {
                return false;
            }
            is_first = false;
            continue;
        }
        if let Some(index) = remaining.find(part) {
            remaining = &remaining[index + part.len()..];
        } else {
            return false;
        }
    }
    pattern.ends_with('*') || remaining.is_empty()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn normalize_permission_alias(input: &str) -> String {
    match input.trim().to_lowercase().replace('-', "_").as_str() {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[tokio::test]
    async fn wait_for_reply_returns_user_response() {
        let bus = EventBus::new();
        let manager = PermissionManager::new(bus);
        let request = manager
            .ask_for_session(Some("ses_1"), "bash", json!({"command":"echo hi"}))
            .await;

        let id = request.id.clone();
        let manager_clone = manager.clone();
        tokio::spawn(async move {
            let _ = manager_clone.reply(&id, "allow").await;
        });

        let cancel = CancellationToken::new();
        let reply = manager.wait_for_reply(&request.id, cancel).await;
        assert_eq!(reply.as_deref(), Some("allow"));
    }

    #[tokio::test]
    async fn wait_for_reply_with_timeout_reports_timeout() {
        let bus = EventBus::new();
        let manager = PermissionManager::new(bus);
        let request = manager
            .ask_for_session(Some("ses_1"), "bash", json!({"command":"sleep 10"}))
            .await;

        let cancel = CancellationToken::new();
        let (reply, timed_out) = manager
            .wait_for_reply_with_timeout(
                &request.id,
                cancel,
                Some(tokio::time::Duration::from_millis(20)),
            )
            .await;
        assert!(reply.is_none());
        assert!(timed_out);
    }

    #[tokio::test]
    async fn permission_asked_event_contains_tool_and_args() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let manager = PermissionManager::new(bus);

        let _ = manager
            .ask_for_session(Some("ses_1"), "read", json!({"path":"README.md"}))
            .await;
        let event = rx.recv().await.expect("event");
        assert_eq!(event.event_type, "permission.asked");
        assert_eq!(
            event
                .properties
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "read"
        );
        assert_eq!(
            event
                .properties
                .get("args")
                .and_then(|v| v.get("path"))
                .and_then(|v| v.as_str())
                .unwrap_or(""),
            "README.md"
        );
    }

    #[tokio::test]
    async fn permission_asked_event_includes_args_integrity_context() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let manager = PermissionManager::new(bus);

        let _ = manager
            .ask_for_session_with_context(
                Some("ses_1"),
                "websearch",
                json!({"query":"meaning of life"}),
                Some(PermissionArgsContext {
                    args_source: "inferred_from_user".to_string(),
                    args_integrity: "recovered".to_string(),
                    query: Some("meaning of life".to_string()),
                }),
            )
            .await;

        let event = rx.recv().await.expect("event");
        assert_eq!(event.event_type, "permission.asked");
        assert_eq!(
            event.properties.get("argsSource").and_then(|v| v.as_str()),
            Some("inferred_from_user")
        );
        assert_eq!(
            event
                .properties
                .get("argsIntegrity")
                .and_then(|v| v.as_str()),
            Some("recovered")
        );
        assert_eq!(
            event.properties.get("query").and_then(|v| v.as_str()),
            Some("meaning of life")
        );
    }

    #[tokio::test]
    async fn evaluate_todo_aliases_as_same_permission() {
        let bus = EventBus::new();
        let manager = PermissionManager::new(bus);
        manager.rules.write().await.push(PermissionRule {
            id: Uuid::new_v4().to_string(),
            permission: "todowrite".to_string(),
            pattern: "todowrite".to_string(),
            action: PermissionAction::Allow,
            created_at_ms: None,
            created_by: None,
            source_request_id: None,
            provenance: None,
        });

        let action = manager.evaluate("todo_write", "todo_write").await;
        assert!(matches!(action, PermissionAction::Allow));
    }

    #[tokio::test]
    async fn evaluate_supports_wildcard_permission_names() {
        let bus = EventBus::new();
        let manager = PermissionManager::new(bus);
        manager.rules.write().await.push(PermissionRule {
            id: Uuid::new_v4().to_string(),
            permission: "mcp*".to_string(),
            pattern: "*".to_string(),
            action: PermissionAction::Allow,
            created_at_ms: None,
            created_by: None,
            source_request_id: None,
            provenance: None,
        });

        let action = manager
            .evaluate(
                "mcp.composio_1.gmail_send_email",
                "mcp.composio_1.gmail_send_email",
            )
            .await;
        assert!(matches!(action, PermissionAction::Allow));
        let unrelated = manager.evaluate("bash", "bash").await;
        assert!(matches!(unrelated, PermissionAction::Ask));
    }

    #[tokio::test]
    async fn always_reply_does_not_create_standing_shell_approval() {
        let manager = PermissionManager::new(EventBus::new());
        let req = manager.ask("bash", "bash").await;
        assert!(manager.reply(&req.id, "always").await);

        // No standing Allow rule is persisted for the shell tool...
        assert!(
            manager.list_rules().await.is_empty(),
            "shell `always` must not create a standing approval rule"
        );
        // ...so the next bash invocation is asked again rather than auto-allowed.
        assert!(matches!(
            manager.evaluate("bash", "bash").await,
            PermissionAction::Ask
        ));
    }

    #[tokio::test]
    async fn always_reply_does_not_create_standing_verify_command_approval() {
        let manager = PermissionManager::new(EventBus::new());
        let req = manager.ask("verify_command", "verify_command").await;
        assert!(manager.reply(&req.id, "always").await);

        assert!(
            manager.list_rules().await.is_empty(),
            "verify_command `always` must not create a standing approval rule"
        );
        assert!(matches!(
            manager.evaluate("verify_command", "verify_command").await,
            PermissionAction::Ask
        ));
    }

    #[tokio::test]
    async fn always_reply_persists_standing_approval_for_non_shell_tool() {
        let manager = PermissionManager::new(EventBus::new());
        let req = manager.ask("read", "read").await;
        assert!(manager.reply(&req.id, "always").await);

        assert!(matches!(
            manager.evaluate("read", "read").await,
            PermissionAction::Allow
        ));
    }

    #[tokio::test]
    async fn deny_reply_still_persists_standing_block_for_shell() {
        let manager = PermissionManager::new(EventBus::new());
        let req = manager.ask("bash", "bash").await;
        assert!(manager.reply(&req.id, "reject").await);

        // Standing *deny* rules remain safe to persist for shell tools.
        assert!(matches!(
            manager.evaluate("bash", "bash").await,
            PermissionAction::Deny
        ));
    }

    #[tokio::test]
    async fn persisted_pending_request_is_failed_on_restart_and_reasked() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("permissions.json");
        let manager = PermissionManager::new_with_state_file(EventBus::new(), path.clone())
            .await
            .expect("manager");
        let req = manager
            .ask_for_session(Some("ses_1"), "read", json!({"path":"README.md"}))
            .await;

        let restarted = PermissionManager::new_with_state_file(EventBus::new(), path.clone())
            .await
            .expect("restarted manager");
        let requests = restarted.list().await;
        let recovered = requests
            .iter()
            .find(|candidate| candidate.id == req.id)
            .expect("persisted request");
        assert_eq!(recovered.status, "runtime_restarted");
        assert!(matches!(
            restarted.evaluate("read", "read").await,
            PermissionAction::Ask
        ));
        assert!(restarted
            .list_decisions()
            .await
            .iter()
            .any(|decision| decision.request_id == req.id
                && decision.decision == "runtime_restarted"));

        let decision_count = restarted.list_decisions().await.len();
        assert!(restarted
            .reply_with_provenance(
                &req.id,
                "always",
                Some("alice".to_string()),
                Some("late approval from stale prompt".to_string()),
            )
            .await
            .is_none());
        assert_eq!(restarted.list_decisions().await.len(), decision_count);
        assert!(matches!(
            restarted.evaluate("read", "read").await,
            PermissionAction::Ask
        ));

        let reasked = restarted
            .ask_for_session(Some("ses_1"), "read", json!({"path":"README.md"}))
            .await;
        assert_ne!(reasked.id, req.id);
        assert_eq!(reasked.status, "pending");
    }

    #[tokio::test]
    async fn standing_rules_persist_with_provenance_and_shell_allow_exclusion_survives_restart() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("permissions.json");
        let manager = PermissionManager::new_with_state_file(EventBus::new(), path.clone())
            .await
            .expect("manager");

        let read_req = manager
            .ask_for_session(Some("ses_1"), "read", json!({"path":"README.md"}))
            .await;
        let read_outcome = manager
            .reply_with_provenance(
                &read_req.id,
                "always",
                Some("alice".to_string()),
                Some("approved read access".to_string()),
            )
            .await
            .expect("read reply");
        let standing_rule = read_outcome.rule.expect("standing read rule");
        assert_eq!(standing_rule.created_by.as_deref(), Some("alice"));
        assert_eq!(
            standing_rule.source_request_id.as_deref(),
            Some(read_req.id.as_str())
        );

        let bash_req = manager
            .ask_for_session(Some("ses_1"), "bash", json!({"command":"echo hi"}))
            .await;
        let bash_outcome = manager
            .reply_with_provenance(
                &bash_req.id,
                "always",
                Some("alice".to_string()),
                Some("one-time command approval".to_string()),
            )
            .await
            .expect("bash reply");
        assert!(
            bash_outcome.rule.is_none(),
            "shell always approvals must not persist standing allow rules"
        );

        let restarted = PermissionManager::new_with_state_file(EventBus::new(), path)
            .await
            .expect("restarted manager");
        assert!(matches!(
            restarted.evaluate("read", "read").await,
            PermissionAction::Allow
        ));
        assert!(matches!(
            restarted.evaluate("bash", "bash").await,
            PermissionAction::Ask
        ));
        assert!(restarted
            .list_rules()
            .await
            .iter()
            .any(
                |rule| rule.source_request_id.as_deref() == Some(read_req.id.as_str())
                    && rule.created_by.as_deref() == Some("alice")
            ));
    }

    #[tokio::test]
    async fn concurrent_permission_state_writes_preserve_all_requests() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("permissions.json");
        let manager = Arc::new(
            PermissionManager::new_with_state_file(EventBus::new(), path.clone())
                .await
                .expect("manager"),
        );
        let task_count = 24usize;
        let barrier = Arc::new(tokio::sync::Barrier::new(task_count));
        let mut handles = Vec::with_capacity(task_count);
        for index in 0..task_count {
            let manager = manager.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                manager
                    .ask_for_session(
                        Some("ses_1"),
                        "read",
                        json!({"path": format!("file-{index}.md")}),
                    )
                    .await
                    .id
            }));
        }

        let mut ids = Vec::with_capacity(task_count);
        for handle in handles {
            ids.push(handle.await.expect("permission ask task"));
        }
        let raw = tokio::fs::read_to_string(path)
            .await
            .expect("permission state file");
        let file: PermissionStateFile = serde_json::from_str(&raw).expect("permission state json");
        for id in ids {
            assert!(
                file.requests.contains_key(&id),
                "persisted state should retain request {id}"
            );
        }
    }

    #[test]
    fn standing_allow_is_unsafe_for_every_shell_execution_alias() {
        // Table-driven over the known shell/verify aliases so a new execution
        // tool name cannot silently regain standing "always allow" approval.
        let unsafe_names = [
            "bash",
            "shell",
            "run_command",
            "powershell",
            "cmd",
            "verify_command",
            "verifycommand",
        ];
        for name in unsafe_names {
            assert!(
                standing_allow_is_unsafe(name, name),
                "`{name}` must be excluded from standing allow rules"
            );
            assert!(
                standing_allow_is_unsafe(name, "*"),
                "`{name}` with wildcard pattern must be excluded"
            );
        }

        for name in ["read", "grep", "glob", "webfetch", "todo_write"] {
            assert!(
                !standing_allow_is_unsafe(name, name),
                "`{name}` is not an execution tool and may hold standing rules"
            );
        }
    }
}
