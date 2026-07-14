use super::*;

mod operator_tools_planner_mutations;

use anyhow::{bail, Context};
use std::sync::Arc;
use tandem_plan_compiler::api as compiler_api;
use tandem_types::{
    AccessPermission, ResourceKind, ToolCapabilities, ToolDomain, ToolEffect, ToolRiskTier,
    ToolSecurityDescriptor, VerifiedTenantContext,
};

use crate::app::state::{
    IdempotencyKeyOutcome, IdempotencyReservation, IdempotencyReservationInput,
};
use crate::stateful_runtime::{
    OrchestrationStateStore, StatefulRuntimeStoragePaths, StatefulWaitQuery,
};

const MAX_ARTIFACT_LINKS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperatorToolKind {
    WorkflowStart,
    WorkflowRead,
    WorkflowRevise,
    WorkflowPreview,
    WorkflowValidate,
    WorkflowMaterialize,
    AutomationInspect,
    AutomationDraft,
    AutomationControl,
    OrchestrationInspect,
    Capabilities,
}

impl OperatorToolKind {
    fn name(self) -> &'static str {
        match self {
            Self::WorkflowStart => "workflow_plan_start",
            Self::WorkflowRead => "workflow_plan_read",
            Self::WorkflowRevise => "workflow_plan_revise",
            Self::WorkflowPreview => "workflow_plan_preview",
            Self::WorkflowValidate => "workflow_plan_validate",
            Self::WorkflowMaterialize => "workflow_plan_materialize",
            Self::AutomationInspect => "automation_inspect",
            Self::AutomationDraft => "automation_manage_draft",
            Self::AutomationControl => "automation_control",
            Self::OrchestrationInspect => "orchestration_inspect",
            Self::Capabilities => "workflow_plan_capabilities",
        }
    }

    fn risk_tier(self) -> ToolRiskTier {
        match self {
            Self::WorkflowRead
            | Self::WorkflowPreview
            | Self::WorkflowValidate
            | Self::AutomationInspect
            | Self::OrchestrationInspect
            | Self::Capabilities => ToolRiskTier::ReadDiscover,
            Self::AutomationControl => ToolRiskTier::ConsequentialWrite,
            Self::WorkflowStart
            | Self::WorkflowRevise
            | Self::WorkflowMaterialize
            | Self::AutomationDraft => ToolRiskTier::InternalWrite,
        }
    }

    fn effect(self) -> ToolEffect {
        match self.risk_tier() {
            ToolRiskTier::ReadDiscover => ToolEffect::Read,
            _ => ToolEffect::Write,
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::WorkflowStart => {
                "Start a durable, tenant-scoped workflow-planner session from this authenticated chat"
            }
            Self::WorkflowRead => {
                "Read the active or explicitly selected workflow-planner session and its durable links"
            }
            Self::WorkflowRevise => {
                "Queue a revision against an existing workflow-planner draft without losing its history"
            }
            Self::WorkflowPreview => {
                "Compile a deterministic preview package for the current workflow plan"
            }
            Self::WorkflowValidate => {
                "Validate the current workflow plan and return structured blockers"
            }
            Self::WorkflowMaterialize => {
                "Materialize a validated workflow plan into a disabled Automation V2 draft"
            }
            Self::AutomationInspect => {
                "Search or inspect tenant-visible Automation V2 definitions and recent runs"
            }
            Self::AutomationDraft => {
                "Validate, duplicate, or replace a disabled Automation V2 draft"
            }
            Self::AutomationControl => {
                "Disable or archive an Automation V2 definition with explicit approval"
            }
            Self::OrchestrationInspect => {
                "Inspect tenant-visible orchestration drafts, versions, goals, runs, waits, and failures"
            }
            Self::Capabilities => {
                "Inspect safe product capabilities, providers, MCP servers, channels, memory, and workspace context"
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct OperatorTool {
    state: AppState,
    kind: OperatorToolKind,
}

pub(crate) fn operator_tools(state: AppState) -> Vec<Arc<dyn Tool>> {
    [
        OperatorToolKind::WorkflowStart,
        OperatorToolKind::WorkflowRead,
        OperatorToolKind::WorkflowRevise,
        OperatorToolKind::WorkflowPreview,
        OperatorToolKind::WorkflowValidate,
        OperatorToolKind::WorkflowMaterialize,
        OperatorToolKind::AutomationInspect,
        OperatorToolKind::AutomationDraft,
        OperatorToolKind::AutomationControl,
        OperatorToolKind::OrchestrationInspect,
        OperatorToolKind::Capabilities,
    ]
    .into_iter()
    .map(|kind| {
        Arc::new(OperatorTool {
            state: state.clone(),
            kind,
        }) as Arc<dyn Tool>
    })
    .collect()
}

#[async_trait]
impl Tool for OperatorTool {
    fn schema(&self) -> ToolSchema {
        let (properties, required) = match self.kind {
            OperatorToolKind::WorkflowStart => (
                json!({
                    "chat_session_id": { "type": "string" },
                    "prompt": { "type": "string" },
                    "project_slug": { "type": "string" },
                    "title": { "type": "string" },
                    "workspace_root": { "type": "string" },
                    "allowed_mcp_servers": { "type": "array", "items": { "type": "string" } },
                    "idempotency_key": { "type": "string" }
                }),
                vec!["prompt", "idempotency_key"],
            ),
            OperatorToolKind::WorkflowRead
            | OperatorToolKind::WorkflowPreview
            | OperatorToolKind::WorkflowValidate => (
                json!({
                    "chat_session_id": { "type": "string" },
                    "planner_session_id": { "type": "string" }
                }),
                vec![],
            ),
            OperatorToolKind::WorkflowRevise => (
                json!({
                    "chat_session_id": { "type": "string" },
                    "planner_session_id": { "type": "string" },
                    "message": { "type": "string" },
                    "expected_revision": { "type": "integer", "minimum": 1 },
                    "idempotency_key": { "type": "string" }
                }),
                vec!["planner_session_id", "message", "idempotency_key"],
            ),
            OperatorToolKind::WorkflowMaterialize => (
                json!({
                    "chat_session_id": { "type": "string" },
                    "planner_session_id": { "type": "string" },
                    "expected_revision": { "type": "integer", "minimum": 1 },
                    "overlap_decision": { "type": "string", "enum": ["reuse", "merge", "fork", "new"] },
                    "idempotency_key": { "type": "string" }
                }),
                vec!["planner_session_id", "idempotency_key"],
            ),
            OperatorToolKind::AutomationInspect => (
                json!({
                    "chat_session_id": { "type": "string" },
                    "automation_id": { "type": "string" },
                    "query": { "type": "string" },
                    "include_runs": { "type": "boolean" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }),
                vec![],
            ),
            OperatorToolKind::AutomationDraft => (
                json!({
                    "chat_session_id": { "type": "string" },
                    "action": { "type": "string", "enum": ["create", "validate", "duplicate", "revise"] },
                    "automation_id": { "type": "string" },
                    "new_automation_id": { "type": "string" },
                    "name": { "type": "string" },
                    "expected_updated_at_ms": { "type": "integer", "minimum": 0 },
                    "automation": { "type": "object" },
                    "idempotency_key": { "type": "string" }
                }),
                vec!["action", "idempotency_key"],
            ),
            OperatorToolKind::AutomationControl => (
                json!({
                    "chat_session_id": { "type": "string" },
                    "action": { "type": "string", "enum": ["disable", "archive"] },
                    "automation_id": { "type": "string" },
                    "expected_updated_at_ms": { "type": "integer", "minimum": 0 },
                    "reason": { "type": "string" },
                    "idempotency_key": { "type": "string" }
                }),
                vec!["action", "automation_id", "idempotency_key"],
            ),
            OperatorToolKind::OrchestrationInspect => (
                json!({
                    "chat_session_id": { "type": "string" },
                    "resource": { "type": "string", "enum": ["orchestration", "goal", "run", "wait", "failure"] },
                    "resource_id": { "type": "string" },
                    "orchestration_id": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }),
                vec!["resource"],
            ),
            OperatorToolKind::Capabilities => {
                (json!({ "chat_session_id": { "type": "string" } }), vec![])
            }
        };
        let permission = if self.kind.risk_tier() == ToolRiskTier::ReadDiscover {
            AccessPermission::Read
        } else {
            AccessPermission::Execute
        };
        ToolSchema::new(
            self.kind.name(),
            self.kind.description(),
            json!({
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false
            }),
        )
        .with_capabilities(
            ToolCapabilities::new()
                .effect(self.kind.effect())
                .domain(ToolDomain::Planning)
                .requires_verification(),
        )
        .with_security(
            ToolSecurityDescriptor::new()
                .permission(permission)
                .resource_kind(match self.kind {
                    OperatorToolKind::OrchestrationInspect => ResourceKind::Run,
                    _ => ResourceKind::Automation,
                })
                .risk_tier(self.kind.risk_tier()),
        )
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        bail!(
            "{} requires an authenticated tenant context",
            self.kind.name()
        )
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        let dispatch_session_id = required_str(&args, "__dispatch_session_id")?;
        let chat_session_id = optional_str(&args, "chat_session_id").unwrap_or(dispatch_session_id);
        if chat_session_id != dispatch_session_id {
            bail!("chat_session_id must match the authenticated dispatch session");
        }
        let chat_session = scoped_chat_session(&self.state, &tenant, chat_session_id).await?;
        let output = match self.kind {
            OperatorToolKind::WorkflowStart => {
                operator_tools_planner_mutations::workflow_start(
                    &self.state,
                    &args,
                    &tenant,
                    &chat_session,
                )
                .await?
            }
            OperatorToolKind::WorkflowRead => {
                let session = scoped_planner_session(
                    &self.state,
                    &tenant,
                    chat_session_id,
                    optional_str(&args, "planner_session_id"),
                )
                .await?;
                planner_envelope("read", &session, None)
            }
            OperatorToolKind::WorkflowRevise => {
                operator_tools_planner_mutations::workflow_revise(
                    &self.state,
                    &args,
                    &tenant,
                    &chat_session,
                )
                .await?
            }
            OperatorToolKind::WorkflowPreview => {
                workflow_preview(&self.state, &args, &tenant, chat_session_id).await?
            }
            OperatorToolKind::WorkflowValidate => {
                workflow_validate(&self.state, &args, &tenant, chat_session_id).await?
            }
            OperatorToolKind::WorkflowMaterialize => {
                workflow_materialize(&self.state, &args, &tenant, &chat_session).await?
            }
            OperatorToolKind::AutomationInspect => {
                automation_inspect(&self.state, &args, &tenant).await?
            }
            OperatorToolKind::AutomationDraft => {
                automation_draft(&self.state, &args, &tenant, &chat_session).await?
            }
            OperatorToolKind::AutomationControl => {
                automation_control(&self.state, &args, &tenant, &chat_session).await?
            }
            OperatorToolKind::OrchestrationInspect => {
                orchestration_inspect(&self.state, &args, &tenant).await?
            }
            OperatorToolKind::Capabilities => {
                super::operator_tools_context::product_capabilities(
                    &self.state,
                    &tenant,
                    &chat_session,
                )
                .await?
            }
        };
        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string()),
            metadata: output,
        })
    }
}

fn required_str<'a>(args: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .with_context(|| format!("{key} is required"))
}

fn optional_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn tool_call_id(args: &Value) -> Option<String> {
    optional_str(args, "__tool_call_id").map(str::to_string)
}

fn ensure_expected_revision(
    args: &Value,
    session: &super::workflow_planner::WorkflowPlannerSessionRecord,
) -> anyhow::Result<()> {
    let Some(expected) = args.get("expected_revision").and_then(Value::as_u64) else {
        return Ok(());
    };
    let current = session
        .draft
        .as_ref()
        .map(|draft| u64::from(draft.plan_revision))
        .unwrap_or(0);
    if expected != current {
        bail!("workflow revision conflict: expected {expected}, current revision is {current}; read the latest draft before retrying");
    }
    Ok(())
}

fn ensure_expected_automation_update(
    args: &Value,
    automation: &crate::AutomationV2Spec,
) -> anyhow::Result<()> {
    if let Some(expected) = args.get("expected_updated_at_ms").and_then(Value::as_u64) {
        if expected != automation.updated_at_ms {
            bail!("automation update conflict: expected {expected}, current update is {}; inspect the latest automation before retrying", automation.updated_at_ms);
        }
    }
    Ok(())
}

fn operator_args_fingerprint(args: &Value, operation: &str) -> String {
    let mut normalized = args.clone();
    if let Some(object) = normalized.as_object_mut() {
        object.retain(|key, _| !key.starts_with("__"));
    }
    crate::sha256_hex(&[operation, &normalized.to_string()])
}

fn tenant_matches(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

async fn scoped_chat_session(
    state: &AppState,
    tenant: &TenantContext,
    session_id: &str,
) -> anyhow::Result<Session> {
    let session = state
        .storage
        .get_session(session_id)
        .await
        .context("chat session not found")?;
    if super::sessions_actor_scope::ensure_same_session_actor(tenant, &session.tenant_context)
        .is_err()
    {
        bail!("chat session not found");
    }
    Ok(session)
}

fn verified_context(args: &Value) -> Option<VerifiedTenantContext> {
    args.get("__verified_tenant_context")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn mutation_actor(
    args: &Value,
    tenant: &TenantContext,
    chat_session: &Session,
) -> anyhow::Result<(String, Option<VerifiedTenantContext>)> {
    let verified = verified_context(args).or_else(|| chat_session.verified_tenant_context.clone());
    if let Some(verified) = verified.as_ref() {
        if !tenant_matches(&verified.tenant_context, tenant) {
            bail!("verified principal does not match the request tenant");
        }
        let actor = verified.human_actor.actor_id.trim();
        if !actor.is_empty() {
            return Ok((actor.to_string(), Some(verified.clone())));
        }
    }
    if tenant.is_local_implicit() {
        return Ok(("local-operator".to_string(), verified));
    }
    bail!("product mutation requires the authenticated chat principal")
}

fn require_product_control_authority(
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> anyhow::Result<()> {
    if tenant.is_local_implicit() {
        return Ok(());
    }
    let verified = verified.context("product control requires a verified tenant principal")?;
    let role_allowed = verified.roles.iter().any(|role| {
        let role = role.to_ascii_lowercase();
        role.contains("admin") || role.contains("owner") || role.contains("operator")
    });
    let capability_allowed = verified.capabilities.iter().any(|capability| {
        let capability = capability.to_ascii_lowercase();
        (capability.contains("automation") || capability.contains("product"))
            && (capability.contains("control")
                || capability.contains("manage")
                || capability.contains("write"))
    });
    if !role_allowed && !capability_allowed {
        bail!("product control requires an operator role or automation-control capability");
    }
    Ok(())
}

async fn active_chat_run_id(state: &AppState, chat_session_id: &str) -> Option<String> {
    state
        .run_registry
        .get(chat_session_id)
        .await
        .map(|run| run.run_id)
}

fn planner_url(session_id: &str) -> String {
    format!("/#/planner?session_id={session_id}")
}

fn automation_url(automation_id: &str) -> String {
    format!("/#/automations?automation_id={automation_id}")
}

fn orchestration_url(orchestration_id: &str) -> String {
    format!("/#/orchestrations?orchestration_id={orchestration_id}")
}

fn push_artifact_link(
    session: &mut super::workflow_planner::WorkflowPlannerSessionRecord,
    kind: &str,
    resource_id: &str,
    resource_url: String,
    revision: Option<u32>,
    chat_run_id: Option<String>,
    tool_call_id: Option<String>,
    idempotency_key: Option<String>,
    tool_name: &str,
) {
    session.artifact_links.retain(|link| {
        !(link.kind == kind && link.resource_id == resource_id && link.revision == revision)
    });
    session
        .artifact_links
        .push(super::workflow_planner::WorkflowPlannerArtifactLink {
            link_id: format!("operator-link-{}", Uuid::new_v4()),
            kind: kind.to_string(),
            resource_id: resource_id.to_string(),
            resource_url,
            revision,
            chat_run_id,
            tool_name: Some(tool_name.to_string()),
            tool_call_id,
            idempotency_key,
            linked_at_ms: crate::now_ms(),
        });
    session.artifact_links.sort_by_key(|link| link.linked_at_ms);
    if session.artifact_links.len() > MAX_ARTIFACT_LINKS {
        session
            .artifact_links
            .drain(..session.artifact_links.len() - MAX_ARTIFACT_LINKS);
    }
}

async fn scoped_planner_session(
    state: &AppState,
    tenant: &TenantContext,
    chat_session_id: &str,
    planner_session_id: Option<&str>,
) -> anyhow::Result<super::workflow_planner::WorkflowPlannerSessionRecord> {
    if let Some(planner_session_id) = planner_session_id {
        let mut session = state
            .get_workflow_planner_session(planner_session_id)
            .await
            .context("workflow planner session not found")?;
        if !tenant_matches(&session.tenant_context, tenant)
            || session.linked_chat_session_id.as_deref() != Some(chat_session_id)
        {
            bail!("workflow planner session not found");
        }
        session.last_referenced_at_ms = Some(crate::now_ms());
        return state.put_workflow_planner_session(session).await;
    }
    let mut matches = state
        .list_workflow_planner_sessions(None)
        .await
        .into_iter()
        .filter(|session| {
            tenant_matches(&session.tenant_context, tenant)
                && session.linked_chat_session_id.as_deref() == Some(chat_session_id)
        })
        .collect::<Vec<_>>();
    matches.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    match matches.len() {
        0 => bail!("this chat has no linked workflow planner session"),
        1 => Ok(matches.remove(0)),
        _ => bail!("multiple workflow drafts are linked; provide planner_session_id"),
    }
}

fn planner_envelope(
    action: &str,
    session: &super::workflow_planner::WorkflowPlannerSessionRecord,
    extra: Option<Value>,
) -> Value {
    json!({
        "ok": true,
        "action": action,
        "resource": {
            "kind": "workflow_planner_session",
            "id": session.session_id,
            "url": planner_url(&session.session_id),
        },
        "status": session.operation.as_ref().map(|operation| operation.status.as_str()).unwrap_or("ready"),
        "planner_session": session,
        "blockers": session.draft.as_ref()
            .and_then(|draft| draft.review.as_ref())
            .map(|review| review.blocked_capabilities.clone())
            .unwrap_or_default(),
        "extra": extra,
    })
}

async fn reserve_idempotent(
    state: &AppState,
    tenant: &TenantContext,
    operation: &str,
    key: &str,
    owner: &str,
    fingerprint: &str,
) -> anyhow::Result<Option<Value>> {
    match state
        .reserve_idempotency_key(IdempotencyReservationInput {
            tenant_context: tenant.clone(),
            operation: operation.to_string(),
            key: key.to_string(),
            owner: owner.to_string(),
            request_fingerprint: fingerprint.to_string(),
            first_seen_event_id: None,
            now_ms: crate::now_ms(),
            expires_at_ms: None,
        })
        .await?
    {
        IdempotencyReservation::Reserved(_) => Ok(None),
        IdempotencyReservation::Duplicate(record) => Ok(Some(
            record
                .outcome
                .map(|outcome| outcome.details)
                .unwrap_or_else(|| {
                    json!({
                        "ok": true,
                        "status": "in_progress",
                        "idempotency_key": key,
                    })
                }),
        )),
        IdempotencyReservation::Conflict(_) => {
            bail!("idempotency key is already bound to a different request")
        }
    }
}

async fn replay_idempotent(
    state: &AppState,
    tenant: &TenantContext,
    operation: &str,
    key: &str,
    fingerprint: &str,
) -> anyhow::Result<Option<Value>> {
    let Some(record) = state.get_idempotency_key(tenant, operation, key).await else {
        return Ok(None);
    };
    if record.request_fingerprint != fingerprint {
        bail!("idempotency key is already bound to a different request");
    }
    Ok(record.outcome.map(|outcome| outcome.details))
}

async fn complete_idempotent(
    state: &AppState,
    tenant: &TenantContext,
    operation: &str,
    key: &str,
    primary_kind: &str,
    primary_id: &str,
    details: Value,
) -> anyhow::Result<Value> {
    state
        .complete_idempotency_key(
            tenant,
            operation,
            key,
            IdempotencyKeyOutcome {
                outcome_kind: "completed".to_string(),
                completed_at_ms: crate::now_ms(),
                primary_ref_kind: Some(primary_kind.to_string()),
                primary_ref_id: Some(primary_id.to_string()),
                secondary_ref_kind: None,
                secondary_ref_id: None,
                details: details.clone(),
            },
            crate::now_ms(),
        )
        .await?;
    Ok(details)
}

async fn release_idempotent(
    state: &AppState,
    tenant: &TenantContext,
    operation: &str,
    key: &str,
    fingerprint: &str,
) -> anyhow::Result<()> {
    state
        .release_reserved_idempotency_key(tenant, operation, key, fingerprint)
        .await?;
    Ok(())
}

fn idempotency_in_progress(value: &Value) -> bool {
    value.get("status").and_then(Value::as_str) == Some("in_progress")
}

fn compiled_plan(
    session: &super::workflow_planner::WorkflowPlannerSessionRecord,
) -> anyhow::Result<(
    crate::WorkflowPlan,
    compiler_api::PlanPackage,
    compiler_api::PlanValidationReport,
)> {
    let draft = session
        .draft
        .as_ref()
        .context("workflow draft is not ready")?;
    let plan = draft.current_plan.clone();
    compiler_api::validate_workflow_plan(&plan).map_err(anyhow::Error::msg)?;
    let value = compiler_api::workflow_plan_to_json(&plan).map_err(anyhow::Error::msg)?;
    let package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &value,
        Some("agentic_chat"),
        draft.plan_revision,
    );
    let validation = compiler_api::validate_plan_package(&package);
    Ok((plan, package, validation))
}

fn planner_review_blockers(
    session: &super::workflow_planner::WorkflowPlannerSessionRecord,
) -> Vec<Value> {
    let Some(review) = session
        .draft
        .as_ref()
        .and_then(|draft| draft.review.as_ref())
    else {
        return Vec::new();
    };
    let mut blockers = review
        .blocked_capabilities
        .iter()
        .map(|capability| {
            json!({
                "code": "capability_blocked",
                "capability": capability,
                "message": format!("Required capability `{capability}` is unavailable"),
            })
        })
        .collect::<Vec<_>>();
    if matches!(
        review
            .validation_status
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "blocked" | "failed" | "invalid"
    ) {
        blockers.push(json!({
            "code": "planner_validation_blocked",
            "message": "Planner review validation is blocked",
            "validation_status": review.validation_status,
        }));
    }
    blockers
}

fn planner_review_requirements(
    session: &super::workflow_planner::WorkflowPlannerSessionRecord,
) -> Vec<Value> {
    session
        .draft
        .as_ref()
        .and_then(|draft| draft.review.as_ref())
        .filter(|review| !review.approval_status.trim().is_empty())
        .map(|review| {
            vec![json!({
                "code": "planner_approval_status",
                "approval_status": review.approval_status,
                "applies_to": "consequential_activation_or_delivery",
            })]
        })
        .unwrap_or_default()
}

async fn workflow_preview(
    state: &AppState,
    args: &Value,
    tenant: &TenantContext,
    chat_session_id: &str,
) -> anyhow::Result<Value> {
    let session = scoped_planner_session(
        state,
        tenant,
        chat_session_id,
        optional_str(args, "planner_session_id"),
    )
    .await?;
    let (plan, package, validation) = compiled_plan(&session)?;
    Ok(planner_envelope(
        "preview",
        &session,
        Some(json!({
            "plan": plan,
            "plan_package": package,
            "validation": validation,
        })),
    ))
}

async fn workflow_validate(
    state: &AppState,
    args: &Value,
    tenant: &TenantContext,
    chat_session_id: &str,
) -> anyhow::Result<Value> {
    let session = scoped_planner_session(
        state,
        tenant,
        chat_session_id,
        optional_str(args, "planner_session_id"),
    )
    .await?;
    let (_, _, validation) = compiled_plan(&session)?;
    let mut blockers = validation
        .issues
        .iter()
        .filter(|issue| issue.blocking)
        .map(|issue| serde_json::to_value(issue).unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    blockers.extend(planner_review_blockers(&session));
    let requirements = planner_review_requirements(&session);
    let ready = blockers.is_empty();
    Ok(json!({
        "ok": true,
        "action": "validate",
        "status": if ready { "ready_for_materialization" } else { "blocked" },
        "resource": {
            "kind": "workflow_planner_session",
            "id": session.session_id,
            "url": planner_url(&session.session_id),
        },
        "validation": validation,
        "blockers": blockers,
        "requirements": requirements,
    }))
}

async fn workflow_materialize(
    state: &AppState,
    args: &Value,
    tenant: &TenantContext,
    chat_session: &Session,
) -> anyhow::Result<Value> {
    let (_actor, verified) = mutation_actor(args, tenant, chat_session)?;
    let planner_session_id = required_str(args, "planner_session_id")?;
    let key = required_str(args, "idempotency_key")?;
    let mut session =
        scoped_planner_session(state, tenant, &chat_session.id, Some(planner_session_id)).await?;
    ensure_expected_revision(args, &session)?;
    let (plan, _, validation) = compiled_plan(&session)?;
    let mut blockers = validation
        .issues
        .iter()
        .filter(|issue| issue.blocking)
        .map(|issue| serde_json::to_value(issue).unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    blockers.extend(planner_review_blockers(&session));
    let requirements = planner_review_requirements(&session);
    if !blockers.is_empty() {
        return Ok(json!({
            "ok": false,
            "action": "materialize",
            "status": "blocked",
            "resource": {
                "kind": "workflow_planner_session",
                "id": planner_session_id,
                "url": planner_url(planner_session_id),
            },
            "validation": validation,
            "blockers": blockers,
            "requirements": requirements,
        }));
    }
    let input = serde_json::from_value(json!({
        "plan_id": plan.plan_id,
        "overlap_decision": optional_str(args, "overlap_decision"),
        "materialize_as_draft": true,
        "idempotency_key": key,
    }))?;
    let response = super::workflow_planner::workflow_plan_apply(
        State(state.clone()),
        Extension(tenant.clone()),
        verified.map(Extension),
        Json(input),
    )
    .await
    .map_err(|(_, payload)| anyhow::anyhow!(payload.0.to_string()))?;
    let automation = response
        .0
        .get("automation")
        .cloned()
        .context("materialization did not return an automation")?;
    let automation_id = automation
        .get("automation_id")
        .and_then(Value::as_str)
        .context("materialized automation has no id")?
        .to_string();
    let plan_revision = session.draft.as_ref().map(|draft| draft.plan_revision);
    push_artifact_link(
        &mut session,
        "automation",
        &automation_id,
        automation_url(&automation_id),
        plan_revision,
        active_chat_run_id(state, &chat_session.id).await,
        tool_call_id(args),
        Some(key.to_string()),
        "workflow_plan_materialize",
    );
    state.put_workflow_planner_session(session).await?;
    let details = json!({
        "ok": true,
        "action": "materialize",
        "status": "draft_created",
        "resource": {
            "kind": "automation",
            "id": automation_id,
            "url": automation_url(&automation_id),
        },
        "automation": automation,
        "planner_session_id": planner_session_id,
        "blockers": [],
        "requirements": requirements,
    });
    Ok(details)
}

fn scoped_automation(
    automation: crate::AutomationV2Spec,
    tenant: &TenantContext,
) -> anyhow::Result<crate::AutomationV2Spec> {
    if !tenant_matches(&automation.tenant_context(), tenant) {
        bail!("automation not found");
    }
    Ok(automation)
}

async fn automation_inspect(
    state: &AppState,
    args: &Value,
    tenant: &TenantContext,
) -> anyhow::Result<Value> {
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .clamp(1, 100) as usize;
    if let Some(automation_id) = optional_str(args, "automation_id") {
        let automation = scoped_automation(
            state
                .get_automation_v2(automation_id)
                .await
                .context("automation not found")?,
            tenant,
        )?;
        let runs = if args
            .get("include_runs")
            .and_then(Value::as_bool)
            .unwrap_or(true)
        {
            state
                .list_automation_v2_runs_scoped(
                    Some(automation_id),
                    Some(&tenant.org_id),
                    Some(&tenant.workspace_id),
                    limit,
                )
                .await
                .into_iter()
                .filter(|run| run.tenant_context.deployment_id == tenant.deployment_id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        return Ok(json!({
            "ok": true,
            "action": "inspect",
            "resource": { "kind": "automation", "id": automation_id, "url": automation_url(automation_id) },
            "automation": automation,
            "runs": runs,
            "blockers": [],
        }));
    }
    let query = optional_str(args, "query").map(str::to_ascii_lowercase);
    let automations = state
        .list_automations_v2()
        .await
        .into_iter()
        .filter(|automation| tenant_matches(&automation.tenant_context(), tenant))
        .filter(|automation| {
            query.as_ref().is_none_or(|query| {
                automation
                    .automation_id
                    .to_ascii_lowercase()
                    .contains(query)
                    || automation.name.to_ascii_lowercase().contains(query)
                    || automation
                        .description
                        .as_deref()
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains(query)
            })
        })
        .take(limit)
        .map(|automation| {
            json!({
                "automation_id": automation.automation_id,
                "name": automation.name,
                "description": automation.description,
                "status": automation.status,
                "updated_at_ms": automation.updated_at_ms,
                "url": automation_url(&automation.automation_id),
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "ok": true,
        "action": "search",
        "resources": automations,
        "count": automations.len(),
        "blockers": [],
    }))
}

fn automation_blockers(automation: &crate::AutomationV2Spec) -> Vec<Value> {
    let mut blockers = Vec::new();
    if automation.name.trim().is_empty() {
        blockers.push(json!({ "code": "missing_name", "message": "Automation name is required" }));
    }
    if automation.flow.nodes.is_empty() {
        blockers.push(
            json!({ "code": "missing_nodes", "message": "Automation requires at least one node" }),
        );
    }
    for issue in tandem_automation::validate_automation_wait_nodes(automation) {
        blockers.push(serde_json::to_value(issue).unwrap_or(Value::Null));
    }
    blockers
}

async fn automation_draft(
    state: &AppState,
    args: &Value,
    tenant: &TenantContext,
    chat_session: &Session,
) -> anyhow::Result<Value> {
    let (actor, _) = mutation_actor(args, tenant, chat_session)?;
    let action = required_str(args, "action")?;
    let key = required_str(args, "idempotency_key")?;
    let fingerprint = operator_args_fingerprint(args, action);
    if let Some(replay) = replay_idempotent(
        state,
        tenant,
        "operator.automation_draft",
        key,
        &fingerprint,
    )
    .await?
    {
        return Ok(replay);
    }
    let mut automation = match action {
        "duplicate" => {
            let automation_id = required_str(args, "automation_id")?;
            let mut source = scoped_automation(
                state
                    .get_automation_v2(automation_id)
                    .await
                    .context("automation not found")?,
                tenant,
            )?;
            let destination_id = optional_str(args, "new_automation_id")
                .map(str::to_string)
                .unwrap_or_else(|| format!("automation-{}", Uuid::new_v4()));
            if state.get_automation_v2(&destination_id).await.is_some() {
                bail!("destination automation already exists; choose a new automation_id");
            }
            source.automation_id = destination_id;
            source.name = optional_str(args, "name")
                .map(str::to_string)
                .unwrap_or_else(|| format!("Copy of {}", source.name));
            source.created_at_ms = crate::now_ms();
            source
        }
        "create" => {
            let value = args
                .get("automation")
                .context("automation is required for create")?;
            let candidate = serde_json::from_value::<crate::AutomationV2Spec>(value.clone())?;
            if state
                .get_automation_v2(&candidate.automation_id)
                .await
                .is_some()
            {
                bail!("automation already exists; inspect and revise it or choose a new automation_id");
            }
            candidate
        }
        "revise" => {
            let supplied = args
                .get("automation")
                .cloned()
                .map(serde_json::from_value::<crate::AutomationV2Spec>)
                .transpose()?;
            let automation_id = optional_str(args, "automation_id")
                .or_else(|| {
                    supplied
                        .as_ref()
                        .map(|candidate| candidate.automation_id.as_str())
                })
                .context("automation_id is required for revise")?;
            let existing = scoped_automation(
                state
                    .get_automation_v2(automation_id)
                    .await
                    .context("automation not found")?,
                tenant,
            )?;
            ensure_expected_automation_update(args, &existing)?;
            if existing
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("archived_at_ms"))
                .is_some()
            {
                bail!(
                    "archived automations cannot be revised; duplicate it into a new draft instead"
                );
            }
            if let Some(mut candidate) = supplied {
                if candidate.automation_id != existing.automation_id {
                    bail!("revising an automation cannot change its automation_id");
                }
                candidate.created_at_ms = existing.created_at_ms;
                candidate.last_fired_at_ms = existing.last_fired_at_ms;
                candidate
            } else {
                existing
            }
        }
        "validate" => {
            if let Some(value) = args.get("automation") {
                let candidate = serde_json::from_value::<crate::AutomationV2Spec>(value.clone())?;
                if let Some(existing) = state.get_automation_v2(&candidate.automation_id).await {
                    let existing = scoped_automation(existing, tenant)?;
                    ensure_expected_automation_update(args, &existing)?;
                } else if args.get("expected_updated_at_ms").is_some() {
                    bail!("automation update conflict: the expected automation no longer exists");
                }
                candidate
            } else {
                let automation_id = required_str(args, "automation_id")?;
                let existing = scoped_automation(
                    state
                        .get_automation_v2(automation_id)
                        .await
                        .context("automation not found")?,
                    tenant,
                )?;
                ensure_expected_automation_update(args, &existing)?;
                existing
            }
        }
        _ => bail!("unsupported draft action"),
    };
    if let Some(replay) = reserve_idempotent(
        state,
        tenant,
        "operator.automation_draft",
        key,
        &actor,
        &fingerprint,
    )
    .await?
    {
        return Ok(replay);
    }
    automation.set_tenant_context(tenant);
    automation.status = crate::AutomationV2Status::Draft;
    automation.next_fire_at_ms = None;
    automation.creator_id = actor;
    let blockers = automation_blockers(&automation);
    if action == "validate" || !blockers.is_empty() {
        let details = json!({
            "ok": blockers.is_empty(),
            "action": "validate",
            "status": if blockers.is_empty() { "valid" } else { "blocked" },
            "resource": {
                "kind": "automation",
                "id": automation.automation_id,
                "url": automation_url(&automation.automation_id),
            },
            "automation": automation,
            "blockers": blockers,
        });
        return complete_idempotent(
            state,
            tenant,
            "operator.automation_draft",
            key,
            "automation",
            &automation.automation_id,
            details,
        )
        .await;
    }
    let stored = match state.put_automation_v2(automation).await {
        Ok(stored) => stored,
        Err(error) => {
            let _ = release_idempotent(
                state,
                tenant,
                "operator.automation_draft",
                key,
                &fingerprint,
            )
            .await;
            return Err(error);
        }
    };
    let details = json!({
        "ok": true,
        "action": action,
        "status": "draft_saved",
        "resource": {
            "kind": "automation",
            "id": stored.automation_id,
            "url": automation_url(&stored.automation_id),
        },
        "automation": stored,
        "blockers": [],
    });
    complete_idempotent(
        state,
        tenant,
        "operator.automation_draft",
        key,
        "automation",
        &stored.automation_id,
        details,
    )
    .await
}

async fn automation_control(
    state: &AppState,
    args: &Value,
    tenant: &TenantContext,
    chat_session: &Session,
) -> anyhow::Result<Value> {
    let (actor, verified) = mutation_actor(args, tenant, chat_session)?;
    require_product_control_authority(tenant, verified.as_ref())?;
    let action = required_str(args, "action")?;
    let automation_id = required_str(args, "automation_id")?;
    let key = required_str(args, "idempotency_key")?;
    let reason = optional_str(args, "reason").unwrap_or("requested from authenticated chat");
    let fingerprint = operator_args_fingerprint(args, "operator.automation_control");
    if let Some(replay) = replay_idempotent(
        state,
        tenant,
        "operator.automation_control",
        key,
        &fingerprint,
    )
    .await?
    {
        return Ok(replay);
    }
    let mut automation = scoped_automation(
        state
            .get_automation_v2(automation_id)
            .await
            .context("automation not found")?,
        tenant,
    )?;
    ensure_expected_automation_update(args, &automation)?;
    if !matches!(action, "disable" | "archive") {
        bail!("unsupported automation control action");
    }
    if let Some(replay) = reserve_idempotent(
        state,
        tenant,
        "operator.automation_control",
        key,
        &actor,
        &fingerprint,
    )
    .await?
    {
        return Ok(replay);
    }
    let now = crate::now_ms();
    match action {
        "disable" => automation.status = crate::AutomationV2Status::Paused,
        "archive" => {
            automation.status = crate::AutomationV2Status::Draft;
            let metadata = automation.metadata.get_or_insert_with(|| json!({}));
            if !metadata.is_object() {
                *metadata = json!({});
            }
            if let Some(object) = metadata.as_object_mut() {
                object.insert("archived_at_ms".to_string(), json!(now));
                object.insert("archived_by".to_string(), json!(actor));
                object.insert("archive_reason".to_string(), json!(reason));
            }
        }
        _ => bail!("unsupported automation control action"),
    }
    automation.next_fire_at_ms = None;
    let stored = match state.put_automation_v2(automation).await {
        Ok(stored) => stored,
        Err(error) => {
            let _ = release_idempotent(
                state,
                tenant,
                "operator.automation_control",
                key,
                &fingerprint,
            )
            .await;
            return Err(error);
        }
    };
    let details = json!({
        "ok": true,
        "action": action,
        "status": stored.status,
        "resource": { "kind": "automation", "id": stored.automation_id, "url": automation_url(&stored.automation_id) },
        "automation": stored,
        "blockers": [],
    });
    complete_idempotent(
        state,
        tenant,
        "operator.automation_control",
        key,
        "automation",
        automation_id,
        details,
    )
    .await
}

async fn orchestration_inspect(
    state: &AppState,
    args: &Value,
    tenant: &TenantContext,
) -> anyhow::Result<Value> {
    let resource = required_str(args, "resource")?;
    let resource_id = optional_str(args, "resource_id");
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(25)
        .clamp(1, 100) as usize;
    let store = OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path)?;
    let data = match resource {
        "orchestration" => {
            let rows =
                if let Some(id) = resource_id.or_else(|| optional_str(args, "orchestration_id")) {
                    store
                        .list_orchestration_specs(tenant)?
                        .into_iter()
                        .filter(|spec| spec.orchestration_id == id)
                        .collect::<Vec<_>>()
                } else {
                    store
                        .list_orchestration_specs(tenant)?
                        .into_iter()
                        .take(limit)
                        .collect()
                };
            json!(rows)
        }
        "goal" => {
            if let Some(id) = resource_id {
                let goal = store
                    .get_goal_for_tenant(tenant, id)?
                    .context("goal not found")?;
                json!({
                    "goal": goal,
                    "run_links": store.list_goal_run_links_for_tenant(tenant, id)?,
                })
            } else {
                json!(store.list_goals(
                    tenant,
                    None,
                    optional_str(args, "orchestration_id"),
                    limit
                )?)
            }
        }
        "run" | "failure" => {
            let rows = state
                .list_automation_v2_runs_scoped(
                    optional_str(args, "orchestration_id"),
                    Some(&tenant.org_id),
                    Some(&tenant.workspace_id),
                    limit,
                )
                .await
                .into_iter()
                .filter(|run| run.tenant_context.deployment_id == tenant.deployment_id)
                .filter(|run| resource_id.is_none_or(|id| run.run_id == id))
                .filter(|run| {
                    resource != "failure"
                        || matches!(run.status, crate::AutomationRunStatus::Failed)
                })
                .collect::<Vec<_>>();
            json!(rows)
        }
        "wait" => {
            let paths =
                StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
            let rows = crate::stateful_runtime::list_stateful_waits(
                &paths.waits_path,
                tenant,
                StatefulWaitQuery {
                    limit: Some(limit),
                    ..Default::default()
                },
            )
            .into_iter()
            .filter(|wait| resource_id.is_none_or(|id| wait.wait_id == id))
            .collect::<Vec<_>>();
            json!(rows)
        }
        _ => bail!("unsupported orchestration resource"),
    };
    let id = resource_id.or_else(|| optional_str(args, "orchestration_id"));
    Ok(json!({
        "ok": true,
        "action": "inspect",
        "resource": {
            "kind": resource,
            "id": id,
            "url": id.map(orchestration_url),
        },
        "data": data,
        "blockers": [],
    }))
}
