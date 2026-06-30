use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_types::{
    SharedToolProgressSink, TenantContext, ToolResult, ToolSchema, VerifiedTenantContext,
};
use tokio_util::sync::CancellationToken;

use crate::ToolRegistry;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDispatchStatus {
    Succeeded,
    Failed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDispatchPolicyOutcome {
    Allowed,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDispatchSource {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
}

impl ToolDispatchSource {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            session_id: None,
            message_id: None,
            run_id: None,
            node_id: None,
        }
    }

    pub fn session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn message(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    pub fn run(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    pub fn node(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }
}

impl Default for ToolDispatchSource {
    fn default() -> Self {
        Self::new("unspecified")
    }
}

#[derive(Debug, Clone)]
pub struct ToolDispatchPolicyContext {
    pub requested_tool: String,
    pub canonical_tool: Option<String>,
    pub args: Value,
    pub tenant_context: TenantContext,
    pub verified_tenant_context: Option<VerifiedTenantContext>,
    pub source: ToolDispatchSource,
    pub scope_allowlist: Vec<String>,
    pub schema: Option<ToolSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDispatchDecision {
    pub outcome: ToolDispatchPolicyOutcome,
    pub reason: Option<String>,
    pub policy_decision_id: Option<String>,
}

impl ToolDispatchDecision {
    pub fn allow() -> Self {
        Self {
            outcome: ToolDispatchPolicyOutcome::Allowed,
            reason: None,
            policy_decision_id: None,
        }
    }

    pub fn allow_with_id(policy_decision_id: impl Into<String>) -> Self {
        Self {
            outcome: ToolDispatchPolicyOutcome::Allowed,
            reason: None,
            policy_decision_id: Some(policy_decision_id.into()),
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            outcome: ToolDispatchPolicyOutcome::Denied,
            reason: Some(reason.into()),
            policy_decision_id: None,
        }
    }

    pub fn is_allowed(&self) -> bool {
        self.outcome == ToolDispatchPolicyOutcome::Allowed
    }
}

#[async_trait]
pub trait ToolDispatchPolicy: Send + Sync {
    async fn evaluate(
        &self,
        context: ToolDispatchPolicyContext,
    ) -> anyhow::Result<ToolDispatchDecision>;
}

#[derive(Debug, Default)]
pub struct AllowAllToolDispatchPolicy;

#[async_trait]
impl ToolDispatchPolicy for AllowAllToolDispatchPolicy {
    async fn evaluate(
        &self,
        _context: ToolDispatchPolicyContext,
    ) -> anyhow::Result<ToolDispatchDecision> {
        Ok(ToolDispatchDecision::allow())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDispatchLedgerEvent {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool: Option<String>,
    pub tenant_context: TenantContext,
    pub source: ToolDispatchSource,
    pub scope_allowlist: Vec<String>,
    pub policy_outcome: ToolDispatchPolicyOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_id: Option<String>,
    pub status: ToolDispatchStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[async_trait]
pub trait ToolDispatchLedger: Send + Sync {
    async fn record(&self, event: ToolDispatchLedgerEvent);
}

#[derive(Debug, Default)]
pub struct NoopToolDispatchLedger;

#[async_trait]
impl ToolDispatchLedger for NoopToolDispatchLedger {
    async fn record(&self, _event: ToolDispatchLedgerEvent) {}
}

#[derive(Clone)]
pub struct ToolDispatchContext {
    pub tenant_context: TenantContext,
    pub verified_tenant_context: Option<VerifiedTenantContext>,
    pub source: ToolDispatchSource,
    pub scope_allowlist: Vec<String>,
    pub policy: Arc<dyn ToolDispatchPolicy>,
    pub ledger: Arc<dyn ToolDispatchLedger>,
}

impl ToolDispatchContext {
    pub fn local(source: impl Into<String>) -> Self {
        Self::for_tenant(source, TenantContext::local_implicit())
    }

    pub fn for_tenant(source: impl Into<String>, tenant_context: TenantContext) -> Self {
        Self {
            tenant_context,
            verified_tenant_context: None,
            source: ToolDispatchSource::new(source),
            scope_allowlist: Vec::new(),
            policy: Arc::new(AllowAllToolDispatchPolicy),
            ledger: Arc::new(NoopToolDispatchLedger),
        }
    }

    pub fn with_source(mut self, source: ToolDispatchSource) -> Self {
        self.source = source;
        self
    }

    pub fn with_verified_tenant_context(
        mut self,
        verified_tenant_context: VerifiedTenantContext,
    ) -> Self {
        self.verified_tenant_context = Some(verified_tenant_context);
        self
    }

    pub fn with_scope_allowlist(mut self, scope_allowlist: Vec<String>) -> Self {
        self.scope_allowlist = scope_allowlist;
        self
    }

    pub fn with_policy(mut self, policy: Arc<dyn ToolDispatchPolicy>) -> Self {
        self.policy = policy;
        self
    }

    pub fn with_ledger(mut self, ledger: Arc<dyn ToolDispatchLedger>) -> Self {
        self.ledger = ledger;
        self
    }
}

#[derive(Clone)]
pub struct GovernedToolDispatcher {
    registry: ToolRegistry,
}

impl GovernedToolDispatcher {
    pub fn new(registry: ToolRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }

    pub async fn dispatch(
        &self,
        name: &str,
        args: Value,
        context: ToolDispatchContext,
    ) -> anyhow::Result<ToolResult> {
        self.dispatch_with_cancel_and_progress(name, args, context, CancellationToken::new(), None)
            .await
    }

    pub async fn dispatch_for_tenant(
        &self,
        name: &str,
        args: Value,
        tenant_context: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        self.dispatch(
            name,
            args,
            ToolDispatchContext::for_tenant("default", tenant_context),
        )
        .await
    }

    pub async fn dispatch_local(&self, name: &str, args: Value) -> anyhow::Result<ToolResult> {
        self.dispatch(name, args, ToolDispatchContext::local("local"))
            .await
    }

    pub async fn dispatch_with_cancel_and_progress(
        &self,
        name: &str,
        mut args: Value,
        context: ToolDispatchContext,
        cancel: CancellationToken,
        progress: Option<SharedToolProgressSink>,
    ) -> anyhow::Result<ToolResult> {
        let schema = self.registry.resolve_schema(name).await;
        let canonical_tool = schema.as_ref().map(|schema| schema.name.clone());
        if let Some(reason) = tenant_mismatch_reason(&context.tenant_context, &context) {
            let decision = ToolDispatchDecision::deny(reason.clone());
            self.record(
                name,
                canonical_tool,
                &context,
                &decision,
                ToolDispatchStatus::Blocked,
                Some(reason.clone()),
            )
            .await;
            return Err(anyhow!("ToolDenied {{ reason: TenantScope }}: {reason}"));
        }
        if !context.scope_allowlist.is_empty()
            && !scope_allows_tool(
                &context.scope_allowlist,
                canonical_tool.as_deref().unwrap_or(name),
            )
            && !scope_allows_tool(&context.scope_allowlist, name)
        {
            let reason = format!(
                "ToolDenied {{ reason: ScopeAllowlist }}: tool `{}` is not allowed by this execution scope.",
                canonical_tool.as_deref().unwrap_or(name)
            );
            let decision = ToolDispatchDecision::deny(reason.clone());
            self.record(
                name,
                canonical_tool,
                &context,
                &decision,
                ToolDispatchStatus::Blocked,
                Some(reason.clone()),
            )
            .await;
            return Err(anyhow!(reason));
        }

        let policy_context = ToolDispatchPolicyContext {
            requested_tool: name.to_string(),
            canonical_tool: canonical_tool.clone(),
            args: args.clone(),
            tenant_context: context.tenant_context.clone(),
            verified_tenant_context: context.verified_tenant_context.clone(),
            source: context.source.clone(),
            scope_allowlist: context.scope_allowlist.clone(),
            schema,
        };
        let decision = match context.policy.evaluate(policy_context).await {
            Ok(decision) => decision,
            Err(err) => {
                let reason = err.to_string();
                let decision = ToolDispatchDecision::deny(reason.clone());
                self.record(
                    name,
                    canonical_tool,
                    &context,
                    &decision,
                    ToolDispatchStatus::Blocked,
                    Some(reason.clone()),
                )
                .await;
                return Err(anyhow!("ToolDenied {{ reason: Policy }}: {reason}"));
            }
        };

        if !decision.is_allowed() {
            let reason = decision
                .reason
                .clone()
                .unwrap_or_else(|| "tool dispatch denied by policy".to_string());
            self.record(
                name,
                canonical_tool,
                &context,
                &decision,
                ToolDispatchStatus::Blocked,
                Some(reason.clone()),
            )
            .await;
            return Err(anyhow!("ToolDenied {{ reason: Policy }}: {reason}"));
        }

        if let Value::Object(object) = &mut args {
            object.remove("__strict_tenant_context");
            object.remove("__verified_tenant_context");
            object.remove("__phase_tool_authority");
            object.remove("__phaseToolAuthority");
            object.remove("__workflow_phase");
            object.remove("__workflowPhase");
            if let Some(verified) = context.verified_tenant_context.as_ref() {
                object
                    .entry("__verified_tenant_context")
                    .or_insert_with(|| serde_json::to_value(verified).unwrap_or(Value::Null));
            }
            if let Some(authority) = phase_tool_authority_from_dispatch_context(&context) {
                object.insert("__phase_tool_authority".to_string(), authority);
            }
        }

        let result = self
            .registry
            .execute_with_cancel_and_progress_for_tenant(
                name,
                args,
                context.tenant_context.clone(),
                cancel,
                progress,
            )
            .await;
        match result {
            Ok(result) => {
                self.record(
                    name,
                    canonical_tool,
                    &context,
                    &decision,
                    ToolDispatchStatus::Succeeded,
                    None,
                )
                .await;
                Ok(result)
            }
            Err(err) => {
                let error = err.to_string();
                self.record(
                    name,
                    canonical_tool,
                    &context,
                    &decision,
                    ToolDispatchStatus::Failed,
                    Some(error.clone()),
                )
                .await;
                Err(err)
            }
        }
    }

    async fn record(
        &self,
        name: &str,
        canonical_tool: Option<String>,
        context: &ToolDispatchContext,
        decision: &ToolDispatchDecision,
        status: ToolDispatchStatus,
        error: Option<String>,
    ) {
        context
            .ledger
            .record(ToolDispatchLedgerEvent {
                tool: name.to_string(),
                canonical_tool,
                tenant_context: context.tenant_context.clone(),
                source: context.source.clone(),
                scope_allowlist: context.scope_allowlist.clone(),
                policy_outcome: decision.outcome.clone(),
                policy_decision_id: decision.policy_decision_id.clone(),
                status,
                error,
            })
            .await;
    }
}

fn tenant_mismatch_reason(
    tenant_context: &TenantContext,
    context: &ToolDispatchContext,
) -> Option<String> {
    let verified = context.verified_tenant_context.as_ref()?;
    let verified_tenant = &verified.tenant_context;
    if tenant_context.org_id != verified_tenant.org_id
        || tenant_context.workspace_id != verified_tenant.workspace_id
        || tenant_context.deployment_id != verified_tenant.deployment_id
    {
        return Some(format!(
            "verified tenant `{}/{}` does not match dispatch tenant `{}/{}`",
            verified_tenant.org_id,
            verified_tenant.workspace_id,
            tenant_context.org_id,
            tenant_context.workspace_id
        ));
    }
    if tenant_context.actor_id.is_some()
        && verified_tenant.actor_id.is_some()
        && tenant_context.actor_id != verified_tenant.actor_id
    {
        return Some("verified actor does not match dispatch actor".to_string());
    }
    None
}

fn phase_tool_authority_from_dispatch_context(context: &ToolDispatchContext) -> Option<Value> {
    if context.scope_allowlist.is_empty()
        && context.source.session_id.is_none()
        && context.source.message_id.is_none()
        && context.source.run_id.is_none()
        && context.source.node_id.is_none()
    {
        return None;
    }

    Some(json!({
        "allowed_tools": context.scope_allowlist,
        "session_id": context.source.session_id,
        "message_id": context.source.message_id,
        "run_id": context.source.run_id,
        "node_id": context.source.node_id,
        "policy_id": "workflow_phase_tool_authority",
        "source": "tool_dispatch_context",
    }))
}

fn scope_allows_tool(patterns: &[String], tool_name: &str) -> bool {
    let tool_name = tool_name.trim().to_ascii_lowercase();
    patterns.iter().any(|pattern| {
        let pattern = pattern.trim().to_ascii_lowercase();
        if pattern.is_empty() {
            return false;
        }
        if pattern == "*" {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix('*') {
            return tool_name.starts_with(prefix);
        }
        pattern == tool_name
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::Tool;
    use serde_json::json;
    use tandem_types::{
        AuthorityChain, HumanActor, RequestPrincipal, ToolCapabilities, ToolDomain, ToolSchema,
        ToolSecurityDescriptor,
    };

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "echo_test".to_string(),
                description: "echo".to_string(),
                input_schema: json!({ "type": "object" }),
                capabilities: ToolCapabilities {
                    domains: vec![ToolDomain::Planning],
                    ..ToolCapabilities::default()
                },
                security: ToolSecurityDescriptor::default(),
            }
        }

        async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: args.to_string(),
                metadata: json!({ "ok": true }),
            })
        }
    }

    #[derive(Default)]
    struct RecordingLedger {
        events: Mutex<Vec<ToolDispatchLedgerEvent>>,
    }

    #[async_trait]
    impl ToolDispatchLedger for RecordingLedger {
        async fn record(&self, event: ToolDispatchLedgerEvent) {
            self.events.lock().expect("ledger lock").push(event);
        }
    }

    struct StaticPolicy(ToolDispatchDecision);

    #[async_trait]
    impl ToolDispatchPolicy for StaticPolicy {
        async fn evaluate(
            &self,
            _context: ToolDispatchPolicyContext,
        ) -> anyhow::Result<ToolDispatchDecision> {
            Ok(self.0.clone())
        }
    }

    async fn dispatcher_with_echo() -> GovernedToolDispatcher {
        let registry = ToolRegistry::new();
        registry
            .register_tool("echo_test".to_string(), Arc::new(EchoTool))
            .await;
        GovernedToolDispatcher::new(registry)
    }

    #[tokio::test]
    async fn dispatcher_denies_scope_allowlist_before_execution() {
        let dispatcher = dispatcher_with_echo().await;
        let ledger = Arc::new(RecordingLedger::default());
        let context = ToolDispatchContext::local("test")
            .with_scope_allowlist(vec!["read".to_string()])
            .with_ledger(ledger.clone());

        let err = dispatcher
            .dispatch("echo_test", json!({"value": 1}), context)
            .await
            .expect_err("allowlist should block");
        assert!(err.to_string().contains("ScopeAllowlist"));
        let events = ledger.events.lock().expect("ledger lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, ToolDispatchStatus::Blocked);
        assert_eq!(events[0].scope_allowlist, vec!["read".to_string()]);
    }

    #[tokio::test]
    async fn dispatcher_denies_policy_before_execution() {
        let dispatcher = dispatcher_with_echo().await;
        let ledger = Arc::new(RecordingLedger::default());
        let context = ToolDispatchContext::local("test")
            .with_policy(Arc::new(StaticPolicy(ToolDispatchDecision::deny(
                "not approved",
            ))))
            .with_ledger(ledger.clone());

        let err = dispatcher
            .dispatch("echo_test", json!({"value": 1}), context)
            .await
            .expect_err("policy should block");
        assert!(err.to_string().contains("Policy"));
        let events = ledger.events.lock().expect("ledger lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, ToolDispatchStatus::Blocked);
        assert_eq!(events[0].policy_outcome, ToolDispatchPolicyOutcome::Denied);
    }

    #[tokio::test]
    async fn dispatcher_records_approved_after_policy_hook() {
        let dispatcher = dispatcher_with_echo().await;
        let ledger = Arc::new(RecordingLedger::default());
        let context = ToolDispatchContext::local("test")
            .with_policy(Arc::new(StaticPolicy(ToolDispatchDecision::allow_with_id(
                "approval-1",
            ))))
            .with_ledger(ledger.clone());

        let result = dispatcher
            .dispatch("echo_test", json!({"value": 1}), context)
            .await
            .expect("policy-approved tool should run");
        assert_eq!(result.metadata["ok"], true);
        let events = ledger.events.lock().expect("ledger lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, ToolDispatchStatus::Succeeded);
        assert_eq!(events[0].policy_decision_id.as_deref(), Some("approval-1"));
    }

    #[tokio::test]
    async fn dispatcher_injects_trusted_phase_tool_authority() {
        let dispatcher = dispatcher_with_echo().await;
        let context = ToolDispatchContext::local("test")
            .with_source(
                ToolDispatchSource::new("engine_loop")
                    .session("session-phase")
                    .message("message-phase")
                    .run("run-phase")
                    .node("node-phase"),
            )
            .with_scope_allowlist(vec![
                "echo_test".to_string(),
                "mcp.notion.alice_search".to_string(),
            ]);

        let result = dispatcher
            .dispatch(
                "echo_test",
                json!({
                    "value": 1,
                    "__phase_tool_authority": {
                        "allowed_tools": ["mcp.notion.spoofed"]
                    }
                }),
                context,
            )
            .await
            .expect("tool should run with trusted dispatch authority");
        let payload: Value = serde_json::from_str(&result.output).expect("echoed json");

        let allowed_tools = payload
            .pointer("/__phase_tool_authority/allowed_tools")
            .and_then(Value::as_array)
            .expect("trusted allowed tools");
        assert!(allowed_tools
            .iter()
            .any(|tool| tool.as_str() == Some("mcp.notion.alice_search")));
        assert!(!allowed_tools
            .iter()
            .any(|tool| tool.as_str() == Some("mcp.notion.spoofed")));
        assert_eq!(
            payload
                .pointer("/__phase_tool_authority/run_id")
                .and_then(Value::as_str),
            Some("run-phase")
        );
        assert_eq!(
            payload
                .pointer("/__phase_tool_authority/source")
                .and_then(Value::as_str),
            Some("tool_dispatch_context")
        );
    }

    #[tokio::test]
    async fn dispatcher_denies_verified_tenant_mismatch() {
        let dispatcher = dispatcher_with_echo().await;
        let ledger = Arc::new(RecordingLedger::default());
        let verified_tenant =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let verified = VerifiedTenantContext {
            tenant_context: verified_tenant,
            human_actor: HumanActor {
                actor_id: "user-a".to_string(),
                provider: Some("tandem".to_string()),
                issuer: None,
                subject: None,
                email: None,
            },
            authority_chain: AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                "user-a", "test",
            )),
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: None,
            issuer: "test".to_string(),
            audience: "test".to_string(),
            issued_at_ms: 1,
            expires_at_ms: 2,
            assertion_id: "assertion-1".to_string(),
            assertion_key_id: None,
        };
        let context = ToolDispatchContext::for_tenant(
            "test",
            TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "user-b"),
        )
        .with_verified_tenant_context(verified)
        .with_ledger(ledger.clone());

        let err = dispatcher
            .dispatch("echo_test", json!({"value": 1}), context)
            .await
            .expect_err("tenant mismatch should block");
        assert!(err.to_string().contains("TenantScope"));
        let events = ledger.events.lock().expect("ledger lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, ToolDispatchStatus::Blocked);
    }
}
