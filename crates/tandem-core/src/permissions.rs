use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::{watch, Mutex, RwLock};
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tandem_types::{EngineEvent, TenantContext};

use crate::event_bus::EventBus;

const PERMISSION_STATE_SCHEMA_VERSION: u32 = 2;
const PERMISSION_REQUEST_TTL_MS: u64 = 15 * 60 * 1000;

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
    #[serde(default = "TenantContext::local_implicit", rename = "tenantContext")]
    pub tenant_context: TenantContext,
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
    #[serde(default = "TenantContext::local_implicit", rename = "tenantContext")]
    pub tenant_context: TenantContext,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "requestedBy"
    )]
    pub requested_by: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        rename = "actionDigest"
    )]
    pub action_digest: String,
    #[serde(default, rename = "expiresAtMs")]
    pub expires_at_ms: u64,
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
    #[serde(default = "TenantContext::local_implicit", rename = "tenantContext")]
    pub tenant_context: TenantContext,
    #[serde(
        default,
        skip_serializing_if = "String::is_empty",
        rename = "actionDigest"
    )]
    pub action_digest: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionReplyError {
    Expired,
    ActionMismatch,
    SessionMismatch,
    PersistenceFailed,
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

fn permission_tenant_matches(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

fn permission_action_digest(
    tenant_context: &TenantContext,
    session_id: Option<&str>,
    permission: &str,
    pattern: &str,
    tool: Option<&str>,
    args: Option<&Value>,
) -> String {
    let payload = json!({
        "tenant": tenant_context,
        "sessionID": session_id,
        "permission": permission,
        "pattern": pattern,
        "tool": tool,
        "args": args,
    });
    format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&payload).unwrap_or_default())
    )
}

fn permission_request_digest(request: &PermissionRequest) -> String {
    permission_action_digest(
        &request.tenant_context,
        request.session_id.as_deref(),
        &request.permission,
        &request.pattern,
        request.tool.as_deref(),
        request.args.as_ref(),
    )
}

pub fn permission_requires_independent_review(request: &PermissionRequest) -> bool {
    [
        request.permission.as_str(),
        request.pattern.as_str(),
        request.tool.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .any(|name| {
        standing_allow_is_unsafe(name, name)
            || matches!(
                normalize_permission_alias(name).as_str(),
                "data_boundary_egress" | "git_push" | "provider_config" | "channel_config"
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
                    tenant_context: request.tenant_context.clone(),
                    action_digest: request.action_digest.clone(),
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

    pub async fn evaluate_for_tenant(
        &self,
        tenant_context: &TenantContext,
        permission: &str,
        pattern: &str,
    ) -> PermissionAction {
        let permission = normalize_permission_alias(permission);
        let pattern = normalize_permission_alias(pattern);
        let rules = self.rules.read().await;
        let matches_rule = |rule: &&PermissionRule| {
            permission_tenant_matches(&rule.tenant_context, tenant_context)
                && wildcard_matches(&normalize_permission_alias(&rule.permission), &permission)
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

    pub async fn evaluate(&self, permission: &str, pattern: &str) -> PermissionAction {
        self.evaluate_for_tenant(&TenantContext::local_implicit(), permission, pattern)
            .await
    }

    /// Convenience wrapper for the common case where both the permission name
    /// and the match pattern are the same tool name. Prefer this over
    /// `evaluate(&tool, &tool)` at call sites to make the intent explicit.
    pub async fn evaluate_tool(&self, tool_name: &str) -> PermissionAction {
        self.evaluate(tool_name, tool_name).await
    }

    pub async fn evaluate_tool_for_tenant(
        &self,
        tenant_context: &TenantContext,
        tool_name: &str,
    ) -> PermissionAction {
        self.evaluate_for_tenant(tenant_context, tool_name, tool_name)
            .await
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

    pub async fn ask_for_session_for_tenant(
        &self,
        tenant_context: &TenantContext,
        session_id: Option<&str>,
        tool: &str,
        args: Value,
    ) -> PermissionRequest {
        self.ask_for_session_with_context_for_tenant(tenant_context, session_id, tool, args, None)
            .await
    }

    pub async fn ask_for_session_with_context(
        &self,
        session_id: Option<&str>,
        tool: &str,
        args: Value,
        context: Option<PermissionArgsContext>,
    ) -> PermissionRequest {
        self.ask_for_session_with_context_for_tenant(
            &TenantContext::local_implicit(),
            session_id,
            tool,
            args,
            context,
        )
        .await
    }

    pub async fn ask_for_session_with_context_for_tenant(
        &self,
        tenant_context: &TenantContext,
        session_id: Option<&str>,
        tool: &str,
        args: Value,
        context: Option<PermissionArgsContext>,
    ) -> PermissionRequest {
        let requested_at_ms = now_ms();
        let action_digest = permission_action_digest(
            tenant_context,
            session_id,
            tool,
            tool,
            Some(tool),
            Some(&args),
        );
        let req = PermissionRequest {
            id: Uuid::new_v4().to_string(),
            tenant_context: tenant_context.clone(),
            requested_by: tenant_context.actor_id.clone(),
            action_digest,
            expires_at_ms: requested_at_ms.saturating_add(PERMISSION_REQUEST_TTL_MS),
            session_id: session_id.map(ToString::to_string),
            permission: tool.to_string(),
            pattern: tool.to_string(),
            tool: Some(tool.to_string()),
            args: Some(args.clone()),
            args_source: context.as_ref().map(|c| c.args_source.clone()),
            args_integrity: context.as_ref().map(|c| c.args_integrity.clone()),
            query: context.as_ref().and_then(|c| c.query.clone()),
            status: "pending".to_string(),
            requested_at_ms,
            decided_at_ms: None,
            decided_by: None,
            decision_reason: None,
        };
        let (tx, _rx) = watch::channel(None);
        let transaction_guard = self.state_write_lock.lock().await;
        self.requests
            .write()
            .await
            .insert(req.id.clone(), req.clone());
        self.waiters.write().await.insert(req.id.clone(), tx);
        if let Err(error) = self.persist_state_unlocked().await {
            self.requests.write().await.remove(&req.id);
            self.waiters.write().await.remove(&req.id);
            drop(transaction_guard);
            tracing::warn!(?error, "failed to persist permission request");
            return req;
        }
        drop(transaction_guard);
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
                "requestedAtMs": req.requested_at_ms,
                "expiresAtMs": req.expires_at_ms,
                "actionDigest": req.action_digest,
                "tenantContext": req.tenant_context
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

    pub async fn list_for_tenant(&self, tenant_context: &TenantContext) -> Vec<PermissionRequest> {
        self.list()
            .await
            .into_iter()
            .filter(|request| permission_tenant_matches(&request.tenant_context, tenant_context))
            .collect()
    }

    pub async fn get_for_tenant(
        &self,
        id: &str,
        tenant_context: &TenantContext,
    ) -> Option<PermissionRequest> {
        self.requests
            .read()
            .await
            .get(id)
            .filter(|request| permission_tenant_matches(&request.tenant_context, tenant_context))
            .cloned()
    }

    pub async fn list_rules(&self) -> Vec<PermissionRule> {
        self.rules.read().await.clone()
    }

    pub async fn list_rules_for_tenant(
        &self,
        tenant_context: &TenantContext,
    ) -> Vec<PermissionRule> {
        self.list_rules()
            .await
            .into_iter()
            .filter(|rule| permission_tenant_matches(&rule.tenant_context, tenant_context))
            .collect()
    }

    pub async fn list_decisions(&self) -> Vec<PermissionDecisionRecord> {
        self.decisions.read().await.clone()
    }

    pub async fn list_decisions_for_tenant(
        &self,
        tenant_context: &TenantContext,
    ) -> Vec<PermissionDecisionRecord> {
        self.list_decisions()
            .await
            .into_iter()
            .filter(|decision| permission_tenant_matches(&decision.tenant_context, tenant_context))
            .collect()
    }

    async fn persist_state(&self) -> anyhow::Result<()> {
        let _write_guard = self.state_write_lock.lock().await;
        self.persist_state_unlocked().await
    }

    async fn persist_state_unlocked(&self) -> anyhow::Result<()> {
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
        self.add_rule_for_tenant(
            &TenantContext::local_implicit(),
            permission,
            pattern,
            action,
        )
        .await
    }

    pub async fn add_rule_for_tenant(
        &self,
        tenant_context: &TenantContext,
        permission: impl Into<String>,
        pattern: impl Into<String>,
        action: PermissionAction,
    ) -> PermissionRule {
        let rule = PermissionRule {
            id: Uuid::new_v4().to_string(),
            tenant_context: tenant_context.clone(),
            permission: permission.into(),
            pattern: pattern.into(),
            action,
            created_at_ms: Some(now_ms()),
            created_by: Some("system".to_string()),
            source_request_id: None,
            provenance: Some("default_or_system_rule".to_string()),
        };
        let transaction_guard = self.state_write_lock.lock().await;
        let mut rules = self.rules.write().await;
        if rules.iter().any(|existing| {
            permission_tenant_matches(&existing.tenant_context, tenant_context)
                && existing.permission == rule.permission
                && existing.pattern == rule.pattern
                && std::mem::discriminant(&existing.action) == std::mem::discriminant(&rule.action)
        }) {
            return rule;
        }
        rules.push(rule.clone());
        drop(rules);
        if let Err(error) = self.persist_state_unlocked().await {
            self.rules
                .write()
                .await
                .retain(|existing| existing.id != rule.id);
            tracing::warn!(?error, "failed to persist permission rule");
        }
        drop(transaction_guard);
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
        self.reply_with_provenance_for_tenant(
            &TenantContext::local_implicit(),
            None,
            id,
            reply,
            decided_by,
            reason,
        )
        .await
        .ok()
        .flatten()
    }

    pub async fn reply_with_provenance_for_tenant(
        &self,
        tenant_context: &TenantContext,
        expected_session_id: Option<&str>,
        id: &str,
        reply: &str,
        decided_by: Option<String>,
        reason: Option<String>,
    ) -> Result<Option<PermissionReplyOutcome>, PermissionReplyError> {
        let transaction_guard = self.state_write_lock.lock().await;
        let before_requests = self.requests.read().await.clone();
        let before_rules = self.rules.read().await.clone();
        let before_decisions = self.decisions.read().await.clone();
        let now = now_ms();
        let request = {
            let mut requests = self.requests.write().await;
            let Some(req) = requests.get_mut(id) else {
                return Ok(None);
            };
            if !permission_tenant_matches(&req.tenant_context, tenant_context) {
                return Ok(None);
            }
            if req.status != "pending" {
                return Ok(None);
            }
            if expected_session_id.is_some() && req.session_id.as_deref() != expected_session_id {
                return Err(PermissionReplyError::SessionMismatch);
            }
            if req.expires_at_ms > 0 && now >= req.expires_at_ms {
                return Err(PermissionReplyError::Expired);
            }
            let expected_digest = permission_request_digest(req);
            if req.action_digest.is_empty() {
                if !tenant_context.is_local_implicit() {
                    return Err(PermissionReplyError::ActionMismatch);
                }
                req.action_digest = expected_digest;
            } else if req.action_digest != expected_digest {
                return Err(PermissionReplyError::ActionMismatch);
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
                    tenant_context: request.tenant_context.clone(),
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
                tenant_context: request.tenant_context.clone(),
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
            tenant_context: request.tenant_context.clone(),
            action_digest: request.action_digest.clone(),
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
        if self.persist_state_unlocked().await.is_err() {
            *self.requests.write().await = before_requests;
            *self.rules.write().await = before_rules;
            *self.decisions.write().await = before_decisions;
            return Err(PermissionReplyError::PersistenceFailed);
        }
        drop(transaction_guard);
        self.event_bus.publish(EngineEvent::new(
            "permission.replied",
            json!({
                "sessionID": request.session_id,
                "requestID": id,
                "reply": reply,
                "decidedAtMs": now,
                "decidedBy": decided_by,
                "standingRuleID": rule.as_ref().map(|rule| rule.id.clone()),
                "standingRulePersisted": rule.is_some(),
                "actionDigest": request.action_digest,
                "tenantContext": request.tenant_context
            }),
        ));
        if let Some(waiter) = self.waiters.read().await.get(id).cloned() {
            let _ = waiter.send(Some(reply.to_string()));
        }
        Ok(Some(PermissionReplyOutcome {
            request,
            decision,
            rule,
        }))
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
    async fn permission_replied_event_preserves_request_session_id() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let manager = PermissionManager::new(bus);

        let request = manager
            .ask_for_session(Some("ses_1"), "read", json!({"path":"README.md"}))
            .await;
        let asked = rx.recv().await.expect("asked event");
        assert_eq!(asked.event_type, "permission.asked");

        assert!(manager.reply(&request.id, "allow").await);
        let replied = rx.recv().await.expect("replied event");
        assert_eq!(replied.event_type, "permission.replied");
        assert_eq!(
            replied
                .properties
                .get("sessionID")
                .and_then(|value| value.as_str()),
            Some("ses_1")
        );
        assert_eq!(
            replied
                .properties
                .get("requestID")
                .and_then(|value| value.as_str()),
            Some(request.id.as_str())
        );
    }

    #[tokio::test]
    async fn evaluate_todo_aliases_as_same_permission() {
        let bus = EventBus::new();
        let manager = PermissionManager::new(bus);
        manager.rules.write().await.push(PermissionRule {
            id: Uuid::new_v4().to_string(),
            tenant_context: TenantContext::local_implicit(),
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
            tenant_context: TenantContext::local_implicit(),
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

    #[tokio::test]
    async fn tenant_scoped_permission_state_cannot_be_discovered_or_decided_cross_tenant() {
        let manager = PermissionManager::new(EventBus::new());
        let tenant_a = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "alice",
        );
        let tenant_b = TenantContext::explicit_user_workspace(
            "org-b",
            "workspace-b",
            Some("deployment-b".to_string()),
            "bob",
        );
        let request = manager
            .ask_for_session_for_tenant(
                &tenant_a,
                Some("session-a"),
                "read",
                json!({"path": "README.md"}),
            )
            .await;

        assert!(manager.list_for_tenant(&tenant_b).await.is_empty());
        assert!(manager
            .get_for_tenant(&request.id, &tenant_b)
            .await
            .is_none());
        assert!(manager
            .reply_with_provenance_for_tenant(
                &tenant_b,
                Some("session-a"),
                &request.id,
                "allow",
                Some("bob".to_string()),
                Some("cross-tenant attempt".to_string()),
            )
            .await
            .expect("cross-tenant lookup")
            .is_none());

        let outcome = manager
            .reply_with_provenance_for_tenant(
                &tenant_a,
                Some("session-a"),
                &request.id,
                "allow",
                Some("reviewer-a".to_string()),
                Some("tenant-local decision".to_string()),
            )
            .await
            .expect("tenant-local reply")
            .expect("pending request");
        assert_eq!(outcome.request.tenant_context, tenant_a);
        assert_eq!(manager.list_decisions_for_tenant(&tenant_a).await.len(), 1);
        assert!(manager
            .list_decisions_for_tenant(&tenant_b)
            .await
            .is_empty());
        assert_eq!(manager.list_rules_for_tenant(&tenant_a).await.len(), 1);
        assert!(manager.list_rules_for_tenant(&tenant_b).await.is_empty());
    }

    #[tokio::test]
    async fn permission_reply_rejects_session_expiry_and_digest_mismatch_without_transition() {
        let manager = PermissionManager::new(EventBus::new());
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "alice",
        );
        let request = manager
            .ask_for_session_for_tenant(
                &tenant,
                Some("session-a"),
                "bash",
                json!({"command": "echo safe"}),
            )
            .await;

        assert!(matches!(
            manager
                .reply_with_provenance_for_tenant(
                    &tenant,
                    Some("session-b"),
                    &request.id,
                    "allow",
                    Some("reviewer".to_string()),
                    None,
                )
                .await,
            Err(PermissionReplyError::SessionMismatch)
        ));
        {
            let mut requests = manager.requests.write().await;
            requests
                .get_mut(&request.id)
                .expect("request")
                .action_digest = "tampered".to_string();
        }
        assert!(matches!(
            manager
                .reply_with_provenance_for_tenant(
                    &tenant,
                    Some("session-a"),
                    &request.id,
                    "allow",
                    Some("reviewer".to_string()),
                    None,
                )
                .await,
            Err(PermissionReplyError::ActionMismatch)
        ));
        {
            let mut requests = manager.requests.write().await;
            let request = requests.get_mut(&request.id).expect("request");
            request.action_digest = permission_request_digest(request);
            request.expires_at_ms = now_ms().saturating_sub(1);
        }
        assert!(matches!(
            manager
                .reply_with_provenance_for_tenant(
                    &tenant,
                    Some("session-a"),
                    &request.id,
                    "allow",
                    Some("reviewer".to_string()),
                    None,
                )
                .await,
            Err(PermissionReplyError::Expired)
        ));
        assert_eq!(
            manager
                .get_for_tenant(&request.id, &tenant)
                .await
                .unwrap()
                .status,
            "pending"
        );
        assert!(manager.list_decisions_for_tenant(&tenant).await.is_empty());
    }

    #[tokio::test]
    async fn permission_persistence_failure_rolls_back_before_notifying_waiter() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("permissions.json");
        let manager = PermissionManager::new_with_state_file(EventBus::new(), path)
            .await
            .expect("manager");
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("deployment-a".to_string()),
            "alice",
        );
        let request = manager
            .ask_for_session_for_tenant(
                &tenant,
                Some("session-a"),
                "read",
                json!({"path": "README.md"}),
            )
            .await;
        *manager.state_path.write().await = Some(dir.path().to_path_buf());

        assert!(matches!(
            manager
                .reply_with_provenance_for_tenant(
                    &tenant,
                    Some("session-a"),
                    &request.id,
                    "allow",
                    Some("reviewer".to_string()),
                    None,
                )
                .await,
            Err(PermissionReplyError::PersistenceFailed)
        ));
        assert_eq!(
            manager
                .get_for_tenant(&request.id, &tenant)
                .await
                .unwrap()
                .status,
            "pending"
        );
        assert!(manager.list_decisions_for_tenant(&tenant).await.is_empty());
        assert!(manager.list_rules_for_tenant(&tenant).await.is_empty());
        let waiter = manager
            .waiters
            .read()
            .await
            .get(&request.id)
            .expect("waiter retained")
            .subscribe();
        assert!(waiter.borrow().is_none());
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
