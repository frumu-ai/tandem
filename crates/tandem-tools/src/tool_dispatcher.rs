use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_types::{
    AccessPermission, DataClass, SharedToolProgressSink, TenantContext, ToolResult, ToolSchema,
    ToolSecurityDescriptor, VerifiedTenantContext,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDispatchReceiptPhase {
    PolicyDecision,
    ExecutionStarted,
    ExecutionCompleted,
    ExecutionFailed,
}

impl ToolDispatchReceiptPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PolicyDecision => "policy_decision",
            Self::ExecutionStarted => "execution_started",
            Self::ExecutionCompleted => "execution_completed",
            Self::ExecutionFailed => "execution_failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::ExecutionCompleted | Self::ExecutionFailed)
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl ToolDispatchSource {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            session_id: None,
            message_id: None,
            run_id: None,
            node_id: None,
            request_id: None,
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

    pub fn request(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    pub fn has_identity_key(&self) -> bool {
        self.session_id.is_some()
            || self.message_id.is_some()
            || self.run_id.is_some()
            || self.node_id.is_some()
            || self.request_id.is_some()
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

    fn is_allow_all(&self) -> bool {
        false
    }
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

    fn is_allow_all(&self) -> bool {
        true
    }
}

#[derive(Debug, Default)]
pub struct DenyAllToolDispatchPolicy;

#[async_trait]
impl ToolDispatchPolicy for DenyAllToolDispatchPolicy {
    async fn evaluate(
        &self,
        context: ToolDispatchPolicyContext,
    ) -> anyhow::Result<ToolDispatchDecision> {
        Ok(ToolDispatchDecision::deny(format!(
            "no dispatch policy matched tool `{}`",
            context
                .canonical_tool
                .as_deref()
                .unwrap_or(&context.requested_tool)
        )))
    }
}

#[derive(Debug)]
struct ExactToolDispatchPolicy {
    tools: Vec<String>,
}

#[async_trait]
impl ToolDispatchPolicy for ExactToolDispatchPolicy {
    async fn evaluate(
        &self,
        context: ToolDispatchPolicyContext,
    ) -> anyhow::Result<ToolDispatchDecision> {
        let requested = context
            .canonical_tool
            .as_deref()
            .unwrap_or(&context.requested_tool);
        Ok(if self.tools.iter().any(|tool| tool == requested) {
            ToolDispatchDecision::allow_with_id("explicit_local_tool_scope")
        } else {
            ToolDispatchDecision::deny(format!(
                "local dispatch scope does not authorize tool `{requested}`"
            ))
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDispatchLedgerEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch_id: Option<String>,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool: Option<String>,
    pub tenant_context: TenantContext,
    pub source: ToolDispatchSource,
    pub scope_allowlist: Vec<String>,
    pub policy_outcome: ToolDispatchPolicyOutcome,
    pub receipt_phase: ToolDispatchReceiptPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_digest: Option<String>,
    pub status: ToolDispatchStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDispatchPreSendEvent {
    pub dispatch_id: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_tool: Option<String>,
    pub args: Value,
    pub tenant_context: TenantContext,
    pub source: ToolDispatchSource,
    pub scope_allowlist: Vec<String>,
    pub policy_outcome: ToolDispatchPolicyOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_digest: Option<String>,
    #[serde(default)]
    pub external_side_effect: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_tier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDispatchPreSendReceipt {
    pub outbox_id: String,
    pub idempotency_key: String,
}

#[async_trait]
pub trait ToolDispatchLedger: Send + Sync {
    async fn prepare_pre_send(
        &self,
        _event: ToolDispatchPreSendEvent,
    ) -> anyhow::Result<Option<ToolDispatchPreSendReceipt>> {
        Ok(None)
    }

    async fn record(&self, event: ToolDispatchLedgerEvent) -> anyhow::Result<()>;

    fn is_noop(&self) -> bool {
        false
    }
}

#[derive(Debug, Default)]
pub struct NoopToolDispatchLedger;

#[async_trait]
impl ToolDispatchLedger for NoopToolDispatchLedger {
    async fn record(&self, _event: ToolDispatchLedgerEvent) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_noop(&self) -> bool {
        true
    }
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
            policy: Arc::new(DenyAllToolDispatchPolicy),
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

    pub fn assert_server_governed(&self) -> anyhow::Result<()> {
        if self.policy.is_allow_all() {
            anyhow::bail!("server tool dispatch cannot use an allow-all policy");
        }
        if self.ledger.is_noop() {
            anyhow::bail!("server tool dispatch cannot use a no-op ledger");
        }
        Ok(())
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
        let canonical_tool = self
            .registry
            .resolve_schema(name)
            .await
            .map(|schema| schema.name)
            .unwrap_or_else(|| name.to_string());
        let mut tools = vec![canonical_tool.clone()];
        if canonical_tool == "batch" {
            for call in args["tool_calls"].as_array().into_iter().flatten() {
                let Some(tool) = crate::resolve_batch_call_tool_name(call) else {
                    continue;
                };
                let canonical = self
                    .registry
                    .resolve_schema(&tool)
                    .await
                    .map(|schema| schema.name)
                    .unwrap_or(tool);
                if canonical != "batch" && !tools.contains(&canonical) {
                    tools.push(canonical);
                }
            }
        }
        self.dispatch(
            name,
            args,
            ToolDispatchContext::local("local")
                .with_scope_allowlist(tools.clone())
                .with_policy(Arc::new(ExactToolDispatchPolicy { tools })),
        )
        .await
    }

    pub async fn dispatch_identity_for(
        &self,
        name: &str,
        args: &Value,
        context: &ToolDispatchContext,
    ) -> (String, String) {
        let schema = self.registry.resolve_schema(name).await;
        let args = tool_execution_args(args.clone(), context);
        dispatch_identity(
            name,
            schema.as_ref().map(|schema| schema.name.as_str()),
            context,
            &args,
        )
    }

    pub async fn dispatch_with_cancel_and_progress(
        &self,
        name: &str,
        args: Value,
        context: ToolDispatchContext,
        cancel: CancellationToken,
        progress: Option<SharedToolProgressSink>,
    ) -> anyhow::Result<ToolResult> {
        let schema = self.registry.resolve_schema(name).await;
        let schema_for_pre_send = schema.clone();
        let canonical_tool = schema.as_ref().map(|schema| schema.name.clone());
        let pre_send_risk_tier = pre_send_risk_tier(name, schema_for_pre_send.as_ref());
        let args = tool_execution_args(args, &context);
        let (dispatch_id, payload_digest) =
            dispatch_identity(name, canonical_tool.as_deref(), &context, &args);
        if let Some(reason) = tenant_mismatch_reason(&context.tenant_context, &context) {
            let decision = ToolDispatchDecision::deny(reason.clone());
            self.record(ToolDispatchRecordInput {
                name,
                canonical_tool,
                dispatch_id: Some(dispatch_id.as_str()),
                payload_digest: Some(payload_digest.as_str()),
                context: &context,
                decision: &decision,
                receipt_phase: ToolDispatchReceiptPhase::PolicyDecision,
                status: ToolDispatchStatus::Blocked,
                error: Some(reason.clone()),
            })
            .await?;
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
            self.record(ToolDispatchRecordInput {
                name,
                canonical_tool,
                dispatch_id: Some(dispatch_id.as_str()),
                payload_digest: Some(payload_digest.as_str()),
                context: &context,
                decision: &decision,
                receipt_phase: ToolDispatchReceiptPhase::PolicyDecision,
                status: ToolDispatchStatus::Blocked,
                error: Some(reason.clone()),
            })
            .await?;
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
                self.record(ToolDispatchRecordInput {
                    name,
                    canonical_tool,
                    dispatch_id: Some(dispatch_id.as_str()),
                    payload_digest: Some(payload_digest.as_str()),
                    context: &context,
                    decision: &decision,
                    receipt_phase: ToolDispatchReceiptPhase::PolicyDecision,
                    status: ToolDispatchStatus::Blocked,
                    error: Some(reason.clone()),
                })
                .await?;
                return Err(anyhow!("ToolDenied {{ reason: Policy }}: {reason}"));
            }
        };

        if !decision.is_allowed() {
            let reason = decision
                .reason
                .clone()
                .unwrap_or_else(|| "tool dispatch denied by policy".to_string());
            self.record(ToolDispatchRecordInput {
                name,
                canonical_tool,
                dispatch_id: Some(dispatch_id.as_str()),
                payload_digest: Some(payload_digest.as_str()),
                context: &context,
                decision: &decision,
                receipt_phase: ToolDispatchReceiptPhase::PolicyDecision,
                status: ToolDispatchStatus::Blocked,
                error: Some(reason.clone()),
            })
            .await?;
            return Err(anyhow!("ToolDenied {{ reason: Policy }}: {reason}"));
        }

        self.record(ToolDispatchRecordInput {
            name,
            canonical_tool: canonical_tool.clone(),
            dispatch_id: Some(dispatch_id.as_str()),
            payload_digest: Some(payload_digest.as_str()),
            context: &context,
            decision: &decision,
            receipt_phase: ToolDispatchReceiptPhase::PolicyDecision,
            status: ToolDispatchStatus::Succeeded,
            error: None,
        })
        .await?;

        let pre_send_receipt = match context
            .ledger
            .prepare_pre_send(ToolDispatchPreSendEvent {
                dispatch_id: dispatch_id.clone(),
                tool: name.to_string(),
                canonical_tool: canonical_tool.clone(),
                args: args.clone(),
                tenant_context: context.tenant_context.clone(),
                source: context.source.clone(),
                scope_allowlist: context.scope_allowlist.clone(),
                policy_outcome: decision.outcome.clone(),
                policy_decision_id: decision.policy_decision_id.clone(),
                payload_digest: Some(payload_digest.clone()),
                external_side_effect: schema_for_pre_send
                    .as_ref()
                    .is_some_and(|schema| schema.security.external_side_effect),
                risk_tier: pre_send_risk_tier,
            })
            .await
        {
            Ok(receipt) => receipt,
            Err(err) => {
                let reason = format!("ToolDenied {{ reason: OutboxGate }}: {err}");
                self.record(ToolDispatchRecordInput {
                    name,
                    canonical_tool,
                    dispatch_id: Some(dispatch_id.as_str()),
                    payload_digest: Some(payload_digest.as_str()),
                    context: &context,
                    decision: &decision,
                    receipt_phase: ToolDispatchReceiptPhase::ExecutionFailed,
                    status: ToolDispatchStatus::Blocked,
                    error: Some(reason.clone()),
                })
                .await?;
                return Err(anyhow!(reason));
            }
        };

        self.record(ToolDispatchRecordInput {
            name,
            canonical_tool: canonical_tool.clone(),
            dispatch_id: Some(dispatch_id.as_str()),
            payload_digest: Some(payload_digest.as_str()),
            context: &context,
            decision: &decision,
            receipt_phase: ToolDispatchReceiptPhase::ExecutionStarted,
            status: ToolDispatchStatus::Succeeded,
            error: None,
        })
        .await?;

        let result = if canonical_tool.as_deref() == Some("batch") || name == "batch" {
            self.execute_governed_batch(args, context.clone(), cancel, progress)
                .await
        } else {
            self.registry
                .execute_with_cancel_and_progress_for_tenant(
                    name,
                    args,
                    context.tenant_context.clone(),
                    cancel,
                    progress,
                )
                .await
        };
        match result {
            Ok(mut result) => {
                let auth_blocked = mcp_auth_required_blocked(&result.metadata);
                let unknown_gated_tool =
                    pre_send_receipt.is_some() && unknown_tool_result(name, &result);
                if !auth_blocked && !unknown_gated_tool {
                    if let Some(receipt) = pre_send_receipt.as_ref() {
                        attach_pre_send_receipt(&mut result, receipt);
                    }
                }
                let unsent_reason = if auth_blocked {
                    Some("MCP authorization required before tool execution".to_string())
                } else if unknown_gated_tool {
                    Some(result.output.clone())
                } else {
                    None
                };
                self.record(ToolDispatchRecordInput {
                    name,
                    canonical_tool,
                    dispatch_id: Some(dispatch_id.as_str()),
                    payload_digest: Some(payload_digest.as_str()),
                    context: &context,
                    decision: &decision,
                    receipt_phase: if unsent_reason.is_some() {
                        ToolDispatchReceiptPhase::ExecutionFailed
                    } else {
                        ToolDispatchReceiptPhase::ExecutionCompleted
                    },
                    status: if unsent_reason.is_some() {
                        ToolDispatchStatus::Blocked
                    } else {
                        ToolDispatchStatus::Succeeded
                    },
                    error: unsent_reason,
                })
                .await?;
                Ok(result)
            }
            Err(err) => {
                let error = err.to_string();
                self.record(ToolDispatchRecordInput {
                    name,
                    canonical_tool,
                    dispatch_id: Some(dispatch_id.as_str()),
                    payload_digest: Some(payload_digest.as_str()),
                    context: &context,
                    decision: &decision,
                    receipt_phase: ToolDispatchReceiptPhase::ExecutionFailed,
                    status: ToolDispatchStatus::Failed,
                    error: Some(error.clone()),
                })
                .await?;
                Err(err)
            }
        }
    }

    async fn record(&self, record: ToolDispatchRecordInput<'_>) -> anyhow::Result<()> {
        record
            .context
            .ledger
            .record(ToolDispatchLedgerEvent {
                dispatch_id: record.dispatch_id.map(str::to_string),
                tool: record.name.to_string(),
                canonical_tool: record.canonical_tool,
                tenant_context: record.context.tenant_context.clone(),
                source: record.context.source.clone(),
                scope_allowlist: record.context.scope_allowlist.clone(),
                policy_outcome: record.decision.outcome.clone(),
                receipt_phase: record.receipt_phase,
                policy_decision_id: record.decision.policy_decision_id.clone(),
                payload_digest: record.payload_digest.map(str::to_string),
                status: record.status,
                error: record.error,
            })
            .await
    }

    async fn execute_governed_batch(
        &self,
        args: Value,
        context: ToolDispatchContext,
        cancel: CancellationToken,
        progress: Option<SharedToolProgressSink>,
    ) -> anyhow::Result<ToolResult> {
        let calls = args["tool_calls"].as_array().cloned().unwrap_or_default();
        let mut outputs = Vec::new();
        for call in calls.iter().take(20) {
            if cancel.is_cancelled() {
                break;
            }
            if let Some(reason) = call.get("_blocked").and_then(Value::as_str) {
                let tool = crate::resolve_batch_call_tool_name(call)
                    .unwrap_or_else(|| "unknown".to_string());
                outputs.push(json!({
                    "tool": tool,
                    "status": "skipped",
                    "output": "",
                    "error": reason,
                    "metadata": {}
                }));
                continue;
            }
            let Some(tool) = crate::resolve_batch_call_tool_name(call)
                .filter(|tool| !tool.is_empty() && tool != "batch")
            else {
                continue;
            };
            let call_args = call.get("args").cloned().unwrap_or_else(|| json!({}));
            let result = Box::pin(self.dispatch_with_cancel_and_progress(
                &tool,
                call_args,
                context.clone(),
                cancel.clone(),
                progress.clone(),
            ))
            .await;
            match result {
                Ok(result) => outputs.push(json!({
                    "tool": tool,
                    "status": "ok",
                    "output": result.output,
                    "error": null,
                    "metadata": result.metadata
                })),
                Err(error) => outputs.push(json!({
                    "tool": tool,
                    "status": "error",
                    "output": "",
                    "error": error.to_string(),
                    "metadata": {}
                })),
            }
        }
        Ok(ToolResult {
            output: serde_json::to_string_pretty(&outputs).unwrap_or_default(),
            metadata: json!({ "count": outputs.len() }),
        })
    }
}

struct ToolDispatchRecordInput<'a> {
    name: &'a str,
    canonical_tool: Option<String>,
    dispatch_id: Option<&'a str>,
    payload_digest: Option<&'a str>,
    context: &'a ToolDispatchContext,
    decision: &'a ToolDispatchDecision,
    receipt_phase: ToolDispatchReceiptPhase,
    status: ToolDispatchStatus,
    error: Option<String>,
}

fn tool_execution_args(mut args: Value, context: &ToolDispatchContext) -> Value {
    if let Value::Object(object) = &mut args {
        object.remove("__strict_tenant_context");
        object.remove("__verified_tenant_context");
        object.remove("__phase_tool_authority");
        object.remove("__phaseToolAuthority");
        object.remove("__workflow_phase");
        object.remove("__workflowPhase");
        object.remove("__dispatch_session_id");
        if let Some(session_id) = context.source.session_id.as_deref() {
            object.insert(
                "__dispatch_session_id".to_string(),
                Value::String(session_id.to_string()),
            );
        }
        if let Some(verified) = context.verified_tenant_context.as_ref() {
            object
                .entry("__verified_tenant_context")
                .or_insert_with(|| serde_json::to_value(verified).unwrap_or(Value::Null));
        }
        if let Some(authority) = phase_tool_authority_from_dispatch_context(context) {
            object.insert("__phase_tool_authority".to_string(), authority);
        }
    }
    args
}

fn attach_pre_send_receipt(result: &mut ToolResult, receipt: &ToolDispatchPreSendReceipt) {
    let receipt = json!({
        "outbox_id": receipt.outbox_id.clone(),
        "idempotency_key": receipt.idempotency_key.clone(),
    });
    match &mut result.metadata {
        Value::Object(object) => {
            object.insert("stateful_outbox".to_string(), receipt);
        }
        other => {
            let previous = std::mem::replace(other, Value::Null);
            result.metadata = json!({
                "stateful_outbox": receipt,
                "tool_metadata": previous,
            });
        }
    }
}

fn mcp_auth_required_blocked(metadata: &Value) -> bool {
    let Some(auth) = metadata.get("mcpAuth") else {
        return false;
    };
    auth.get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && auth
            .get("blocked")
            .and_then(Value::as_bool)
            .unwrap_or(false)
}

fn unknown_tool_result(name: &str, result: &ToolResult) -> bool {
    result.output == format!("Unknown tool: {name}")
}

fn dispatch_identity(
    name: &str,
    canonical_tool: Option<&str>,
    context: &ToolDispatchContext,
    args: &Value,
) -> (String, String) {
    let args_string = args.to_string();
    let payload_digest = format!("sha256:{}", sha256_hex(&[args_string.as_str()]));
    let digest = sha256_hex(&[
        name,
        canonical_tool.unwrap_or(""),
        context.source.kind.as_str(),
        context.source.session_id.as_deref().unwrap_or(""),
        context.source.message_id.as_deref().unwrap_or(""),
        context.source.run_id.as_deref().unwrap_or(""),
        context.source.node_id.as_deref().unwrap_or(""),
        context.source.request_id.as_deref().unwrap_or(""),
        context.tenant_context.org_id.as_str(),
        context.tenant_context.workspace_id.as_str(),
        context
            .tenant_context
            .deployment_id
            .as_deref()
            .unwrap_or(""),
        payload_digest.as_str(),
    ]);
    (
        format!("tool-dispatch-{}", short_hash(&digest)),
        payload_digest,
    )
}

fn pre_send_risk_tier(name: &str, schema: Option<&ToolSchema>) -> Option<String> {
    if let Some(risk) = schema.and_then(|schema| schema.security.risk_tier) {
        return Some(risk.as_str().to_string());
    }

    let descriptor = schema.map(|schema| &schema.security);
    let canonical_name = schema.map(|schema| schema.name.as_str()).unwrap_or(name);
    infer_gated_risk_tier(canonical_name, descriptor)
        .or_else(|| {
            if canonical_name == name {
                None
            } else {
                infer_gated_risk_tier(name, descriptor)
            }
        })
        .map(ToString::to_string)
}

fn infer_gated_risk_tier(
    tool_name: &str,
    descriptor: Option<&ToolSecurityDescriptor>,
) -> Option<&'static str> {
    if descriptor.is_some_and(descriptor_looks_credential_admin)
        || tool_name_looks_credential_admin(tool_name)
    {
        return Some("credential_admin");
    }
    if tool_name_looks_money_movement(tool_name) {
        return Some("money_movement_contract");
    }
    if tool_name_looks_destructive(tool_name) {
        return Some("destructive_delete");
    }
    if descriptor.is_some_and(descriptor_looks_financial_access) {
        return Some("financial_record_access");
    }
    if tool_name_looks_external_send(tool_name) {
        return Some("external_send");
    }
    None
}

fn descriptor_looks_credential_admin(descriptor: &ToolSecurityDescriptor) -> bool {
    descriptor.admin_surface
        || descriptor.credential_access
        || descriptor
            .required_permissions
            .contains(&AccessPermission::Admin)
        || descriptor.data_classes.contains(&DataClass::Credential)
}

fn descriptor_looks_financial_access(descriptor: &ToolSecurityDescriptor) -> bool {
    descriptor
        .data_classes
        .contains(&DataClass::FinancialRecord)
        || descriptor.data_classes.contains(&DataClass::Regulated)
}

fn tool_name_looks_credential_admin(tool_name: &str) -> bool {
    let action = mcp_action_name(tool_name);
    let tokens = tool_name_tokens(action.as_deref().unwrap_or(tool_name));
    let compact = tool_name_compact(action.as_deref().unwrap_or(tool_name));
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "admin"
                | "administrator"
                | "credential"
                | "credentials"
                | "secret"
                | "secrets"
                | "token"
                | "tokens"
                | "oauth"
                | "kms"
                | "key"
                | "keys"
        )
    }) || compact.contains("accesstoken")
        || compact.contains("refreshtoken")
        || compact.contains("secretref")
}

fn tool_name_looks_money_movement(tool_name: &str) -> bool {
    let action = mcp_action_name(tool_name);
    let tokens = tool_name_tokens(action.as_deref().unwrap_or(tool_name));
    [
        "payment",
        "payout",
        "fund",
        "funds",
        "transfer",
        "wire",
        "ach",
        "transaction",
        "ledger",
        "refund",
        "reverse",
        "contract",
        "commitment",
        "invoice",
        "billing",
        "quote",
        "order",
    ]
    .iter()
    .any(|needle| tool_name_tokens_contains(&tokens, needle))
}

fn tool_name_looks_destructive(tool_name: &str) -> bool {
    let action = mcp_action_name(tool_name);
    let tokens = tool_name_tokens(action.as_deref().unwrap_or(tool_name));
    ["delete", "remove", "destroy", "wipe", "purge", "drop"]
        .iter()
        .any(|needle| tool_name_tokens_contains(&tokens, needle))
}

fn tool_name_looks_external_send(tool_name: &str) -> bool {
    let action = mcp_action_name(tool_name);
    let tokens = tool_name_tokens(action.as_deref().unwrap_or(tool_name));
    let compact = tool_name_compact(action.as_deref().unwrap_or(tool_name));
    ["send", "deliver", "reply", "post", "publish", "submit"]
        .iter()
        .any(|needle| tool_name_tokens_contains(&tokens, needle))
        || compact.contains("sendemail")
        || compact.contains("emailsend")
        || compact.contains("replyemail")
        || compact.contains("emailreply")
}

fn mcp_action_name(tool_name: &str) -> Option<String> {
    tool_name
        .trim()
        .to_ascii_lowercase()
        .replace('-', "_")
        .strip_prefix("mcp.")
        .and_then(|rest| rest.rsplit('.').next())
        .map(str::trim)
        .filter(|action| !action.is_empty())
        .map(str::to_string)
}

fn tool_name_tokens(tool_name: &str) -> Vec<String> {
    tool_name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>()
}

fn tool_name_tokens_contains(tokens: &[String], needle: &str) -> bool {
    tokens.iter().any(|token| token == needle)
}

fn tool_name_compact(tool_name: &str) -> String {
    tool_name_tokens(tool_name).join("")
}

fn sha256_hex(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn short_hash(hash: &str) -> String {
    hash.chars().take(24).collect()
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

    struct ExternalMcpTool;

    #[async_trait]
    impl Tool for ExternalMcpTool {
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "mcp.gmail.gmail_send_email".to_string(),
                description: "send email".to_string(),
                input_schema: json!({ "type": "object" }),
                capabilities: ToolCapabilities::default(),
                security: ToolSecurityDescriptor::new().external_side_effect(),
            }
        }

        async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: "sent".to_string(),
                metadata: json!({}),
            })
        }
    }

    struct AuthRequiredMcpTool;

    #[async_trait]
    impl Tool for AuthRequiredMcpTool {
        fn schema(&self) -> ToolSchema {
            ToolSchema {
                name: "mcp.gmail.gmail_send_email".to_string(),
                description: "send email".to_string(),
                input_schema: json!({ "type": "object" }),
                capabilities: ToolCapabilities::default(),
                security: ToolSecurityDescriptor::new().external_side_effect(),
            }
        }

        async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: "authorization required".to_string(),
                metadata: json!({
                    "mcpAuth": {
                        "required": true,
                        "blocked": true,
                        "pending": true,
                        "authorizationUrl": "https://auth.example.test/authorize",
                        "message": "Authorize Gmail before sending email."
                    }
                }),
            })
        }
    }

    struct FailingRecordLedger;

    #[async_trait]
    impl ToolDispatchLedger for FailingRecordLedger {
        async fn record(&self, _event: ToolDispatchLedgerEvent) -> anyhow::Result<()> {
            anyhow::bail!("ledger record failed")
        }
    }

    #[derive(Default)]
    struct RecordingLedger {
        events: Mutex<Vec<ToolDispatchLedgerEvent>>,
        pre_send_events: Mutex<Vec<ToolDispatchPreSendEvent>>,
        pre_send_receipt: Mutex<Option<ToolDispatchPreSendReceipt>>,
    }

    #[async_trait]
    impl ToolDispatchLedger for RecordingLedger {
        async fn record(&self, event: ToolDispatchLedgerEvent) -> anyhow::Result<()> {
            self.events.lock().expect("ledger lock").push(event);
            Ok(())
        }

        async fn prepare_pre_send(
            &self,
            event: ToolDispatchPreSendEvent,
        ) -> anyhow::Result<Option<ToolDispatchPreSendReceipt>> {
            self.pre_send_events
                .lock()
                .expect("ledger lock")
                .push(event);
            Ok(self.pre_send_receipt.lock().expect("ledger lock").clone())
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

    fn allowing_context(source: &str) -> ToolDispatchContext {
        ToolDispatchContext::local(source)
            .with_policy(Arc::new(StaticPolicy(ToolDispatchDecision::allow())))
    }

    #[tokio::test]
    async fn dispatcher_denies_by_default_and_records_the_decision() {
        let dispatcher = dispatcher_with_echo().await;
        let ledger = Arc::new(RecordingLedger::default());
        let context = ToolDispatchContext::for_tenant(
            "server_test",
            TenantContext::explicit("org-a", "workspace-a", None),
        )
        .with_ledger(ledger.clone());

        let err = dispatcher
            .dispatch("echo_test", json!({"value": 1}), context)
            .await
            .expect_err("a missing policy must deny by default");

        assert!(err.to_string().contains("no dispatch policy matched"));
        let events = ledger.events.lock().expect("ledger lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, ToolDispatchStatus::Blocked);
        assert_eq!(events[0].policy_outcome, ToolDispatchPolicyOutcome::Denied);
    }

    #[test]
    fn server_governance_guard_rejects_allow_all_or_noop_ledger() {
        let tenant = TenantContext::explicit("org-a", "workspace-a", None);
        let real_ledger = Arc::new(RecordingLedger::default());
        assert!(ToolDispatchContext::for_tenant("server", tenant.clone())
            .with_ledger(real_ledger.clone())
            .assert_server_governed()
            .is_ok());
        assert!(ToolDispatchContext::for_tenant("server", tenant.clone())
            .with_policy(Arc::new(AllowAllToolDispatchPolicy))
            .with_ledger(real_ledger)
            .assert_server_governed()
            .is_err());
        assert!(ToolDispatchContext::for_tenant("server", tenant)
            .assert_server_governed()
            .is_err());
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
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0].receipt_phase,
            ToolDispatchReceiptPhase::PolicyDecision
        );
        assert_eq!(
            events[1].receipt_phase,
            ToolDispatchReceiptPhase::ExecutionStarted
        );
        assert_eq!(
            events[2].receipt_phase,
            ToolDispatchReceiptPhase::ExecutionCompleted
        );
        assert!(events
            .iter()
            .all(|event| event.policy_decision_id.as_deref() == Some("approval-1")));
    }

    #[tokio::test]
    async fn dispatcher_records_mcp_auth_required_result_as_blocked() {
        let registry = ToolRegistry::new();
        registry
            .register_tool(
                "mcp.gmail.gmail_send_email".to_string(),
                Arc::new(AuthRequiredMcpTool),
            )
            .await;
        let dispatcher = GovernedToolDispatcher::new(registry);
        let ledger = Arc::new(RecordingLedger {
            pre_send_receipt: Mutex::new(Some(ToolDispatchPreSendReceipt {
                outbox_id: "outbox-auth-blocked".to_string(),
                idempotency_key: "tool-dispatch-auth-blocked".to_string(),
            })),
            ..RecordingLedger::default()
        });
        let context = allowing_context("test").with_ledger(ledger.clone());

        let result = dispatcher
            .dispatch(
                "mcp.gmail.gmail_send_email",
                json!({ "to": "a@example.test" }),
                context,
            )
            .await
            .expect("auth-required tool result should be returned");
        assert_eq!(
            result
                .metadata
                .pointer("/mcpAuth/blocked")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert!(result.metadata.get("stateful_outbox").is_none());

        let events = ledger.events.lock().expect("ledger lock");
        assert_eq!(events.len(), 3);
        let terminal = events.last().expect("terminal receipt");
        assert_eq!(
            terminal.receipt_phase,
            ToolDispatchReceiptPhase::ExecutionFailed
        );
        assert_eq!(terminal.status, ToolDispatchStatus::Blocked);
        assert!(terminal
            .error
            .as_deref()
            .is_some_and(|error| error.contains("MCP authorization required")));
    }

    #[tokio::test]
    async fn dispatcher_records_gated_unknown_tool_result_as_blocked() {
        let dispatcher = GovernedToolDispatcher::new(ToolRegistry::new());
        let ledger = Arc::new(RecordingLedger {
            pre_send_receipt: Mutex::new(Some(ToolDispatchPreSendReceipt {
                outbox_id: "outbox-unknown-tool".to_string(),
                idempotency_key: "tool-dispatch-unknown-tool".to_string(),
            })),
            ..RecordingLedger::default()
        });
        let context = allowing_context("test").with_ledger(ledger.clone());

        let result = dispatcher
            .dispatch(
                "mcp.gmail.gmail_send_email",
                json!({ "to": "a@example.test" }),
                context,
            )
            .await
            .expect("unknown tool result should be returned");
        assert_eq!(result.output, "Unknown tool: mcp.gmail.gmail_send_email");
        assert!(result.metadata.get("stateful_outbox").is_none());

        let events = ledger.events.lock().expect("ledger lock");
        assert_eq!(events.len(), 3);
        let terminal = events.last().expect("terminal receipt");
        assert_eq!(
            terminal.receipt_phase,
            ToolDispatchReceiptPhase::ExecutionFailed
        );
        assert_eq!(terminal.status, ToolDispatchStatus::Blocked);
        assert!(terminal
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Unknown tool")));
    }

    #[tokio::test]
    async fn dispatcher_fails_closed_when_required_allow_receipt_fails() {
        let dispatcher = dispatcher_with_echo().await;
        let context = allowing_context("test").with_ledger(Arc::new(FailingRecordLedger));

        let err = dispatcher
            .dispatch("echo_test", json!({"value": 1}), context)
            .await
            .expect_err("record failure should fail closed");
        assert!(err.to_string().contains("ledger record failed"));
    }

    #[tokio::test]
    async fn dispatcher_returns_error_when_required_denial_receipt_fails() {
        let dispatcher = dispatcher_with_echo().await;
        let context = ToolDispatchContext::local("test").with_ledger(Arc::new(FailingRecordLedger));

        let err = dispatcher
            .dispatch("echo_test", json!({"value": 1}), context)
            .await
            .expect_err("a denial without its required receipt must fail closed");
        assert!(err.to_string().contains("ledger record failed"));
    }

    #[tokio::test]
    async fn pre_send_event_infers_mcp_external_send_risk_tier() {
        let registry = ToolRegistry::new();
        registry
            .register_tool(
                "mcp.gmail.gmail_send_email".to_string(),
                Arc::new(ExternalMcpTool),
            )
            .await;
        let dispatcher = GovernedToolDispatcher::new(registry);
        let ledger = Arc::new(RecordingLedger::default());
        let context = allowing_context("test").with_ledger(ledger.clone());

        dispatcher
            .dispatch(
                "mcp.gmail.gmail_send_email",
                json!({"body": "hello"}),
                context,
            )
            .await
            .expect("external send tool dispatch");

        let events = ledger.pre_send_events.lock().expect("ledger lock");
        assert_eq!(events.len(), 1);
        assert!(events[0].external_side_effect);
        assert_eq!(events[0].risk_tier.as_deref(), Some("external_send"));
    }

    #[tokio::test]
    async fn dispatcher_injects_trusted_phase_tool_authority() {
        let dispatcher = dispatcher_with_echo().await;
        let context = allowing_context("test")
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
    async fn batch_subcalls_reenter_policy_and_ledger() {
        let dispatcher = dispatcher_with_echo().await;
        let ledger = Arc::new(RecordingLedger::default());
        let context = allowing_context("batch_test")
            .with_scope_allowlist(vec!["batch".to_string(), "echo_test".to_string()])
            .with_ledger(ledger.clone());

        let result = dispatcher
            .dispatch(
                "batch",
                json!({
                    "tool_calls": [
                        { "tool": "echo_test", "args": { "value": 7 } }
                    ]
                }),
                context,
            )
            .await
            .expect("governed batch dispatch");

        let outputs: Vec<Value> = serde_json::from_str(&result.output).expect("batch output");
        assert_eq!(outputs[0]["status"], "ok");
        let events = ledger.events.lock().expect("ledger lock");
        assert_eq!(
            events.len(),
            6,
            "each inner call and the batch wrapper receive three lifecycle receipts"
        );
        for tool in ["echo_test", "batch"] {
            assert!(events.iter().any(|event| {
                event.tool == tool
                    && event.receipt_phase == ToolDispatchReceiptPhase::PolicyDecision
            }));
            assert!(events.iter().any(|event| {
                event.tool == tool
                    && event.receipt_phase == ToolDispatchReceiptPhase::ExecutionStarted
            }));
            assert!(events.iter().any(|event| {
                event.tool == tool
                    && event.receipt_phase == ToolDispatchReceiptPhase::ExecutionCompleted
            }));
        }
    }

    #[tokio::test]
    async fn mixed_batch_denies_out_of_scope_subcalls_without_stopping_allowed_calls() {
        let dispatcher = dispatcher_with_echo().await;
        let ledger = Arc::new(RecordingLedger::default());
        let context = allowing_context("mixed_batch_test")
            .with_scope_allowlist(vec!["batch".to_string(), "echo_test".to_string()])
            .with_ledger(ledger.clone());

        let result = dispatcher
            .dispatch(
                "batch",
                json!({
                    "tool_calls": [
                        { "tool": "read", "args": { "path": "Cargo.toml" } },
                        { "tool": "echo_test", "args": { "value": 9 } }
                    ]
                }),
                context,
            )
            .await
            .expect("mixed batch dispatch");

        let outputs: Vec<Value> = serde_json::from_str(&result.output).expect("batch output");
        assert_eq!(outputs[0]["status"], "error");
        assert!(outputs[0]["error"]
            .as_str()
            .unwrap_or_default()
            .contains("ScopeAllowlist"));
        assert_eq!(outputs[1]["status"], "ok");
        let events = ledger.events.lock().expect("ledger lock");
        assert!(events.iter().any(|event| {
            event.tool == "read"
                && event.status == ToolDispatchStatus::Blocked
                && event.policy_outcome == ToolDispatchPolicyOutcome::Denied
        }));
        assert!(events.iter().any(|event| {
            event.tool == "echo_test" && event.status == ToolDispatchStatus::Succeeded
        }));
    }

    #[tokio::test]
    async fn dispatch_identity_uses_sanitized_execution_args() {
        let dispatcher = dispatcher_with_echo().await;
        let context = ToolDispatchContext::local("test")
            .with_source(
                ToolDispatchSource::new("engine_loop")
                    .session("session-phase")
                    .message("message-phase")
                    .run("run-phase")
                    .node("node-phase"),
            )
            .with_scope_allowlist(vec!["echo_test".to_string()]);

        let (dispatch_a, payload_a) = dispatcher
            .dispatch_identity_for(
                "echo_test",
                &json!({
                    "value": 1,
                    "__phase_tool_authority": {
                        "allowed_tools": ["mcp.notion.spoofed"]
                    },
                    "__workflow_phase": "draft"
                }),
                &context,
            )
            .await;
        let (dispatch_b, payload_b) = dispatcher
            .dispatch_identity_for(
                "echo_test",
                &json!({
                    "value": 1,
                    "__phaseToolAuthority": {
                        "allowed_tools": ["mcp.github.spoofed"]
                    },
                    "__workflowPhase": "publish"
                }),
                &context,
            )
            .await;

        assert_eq!(payload_a, payload_b);
        assert_eq!(dispatch_a, dispatch_b);
    }

    #[test]
    fn dispatch_identity_includes_deployment_scope() {
        let mut deployment_a =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        deployment_a.deployment_id = Some("deployment-a".to_string());
        let mut deployment_b =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        deployment_b.deployment_id = Some("deployment-b".to_string());
        let source = ToolDispatchSource::new("engine_loop")
            .session("session-1")
            .message("message-1");
        let context_a = ToolDispatchContext::for_tenant("engine_loop", deployment_a)
            .with_source(source.clone());
        let context_b =
            ToolDispatchContext::for_tenant("engine_loop", deployment_b).with_source(source);

        let (dispatch_a, payload_a) =
            dispatch_identity("mcp.github.send", None, &context_a, &json!({"body":"same"}));
        let (dispatch_b, payload_b) =
            dispatch_identity("mcp.github.send", None, &context_b, &json!({"body":"same"}));
        assert_eq!(payload_a, payload_b);
        assert_ne!(dispatch_a, dispatch_b);
    }

    #[test]
    fn dispatch_identity_includes_request_source_key() {
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let context_a = ToolDispatchContext::for_tenant("http_global_tool", tenant.clone())
            .with_source(ToolDispatchSource::new("http_global_tool").request("request-a"));
        let context_b = ToolDispatchContext::for_tenant("http_global_tool", tenant)
            .with_source(ToolDispatchSource::new("http_global_tool").request("request-b"));

        let (dispatch_a, payload_a) =
            dispatch_identity("mcp.github.send", None, &context_a, &json!({"body":"same"}));
        let (dispatch_b, payload_b) =
            dispatch_identity("mcp.github.send", None, &context_b, &json!({"body":"same"}));
        assert_eq!(payload_a, payload_b);
        assert_ne!(dispatch_a, dispatch_b);
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
