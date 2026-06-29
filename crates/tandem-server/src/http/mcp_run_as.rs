use serde_json::{json, Value};
use tandem_runtime::McpPrincipalRef;
use tandem_types::{
    AccessDecision, AccessPermission, DataClass, GrantEvaluation, PolicyDecisionEffect,
    PolicyDecisionRecord, PrincipalRef, ResourceKind, ResourcePathSegment, ResourceRef,
    StrictTenantContext, TenantContext, ToolResult, VerifiedTenantContext,
};

use crate::{now_ms, AppState};

const MCP_CONNECTION_ID_ARG: &str = "__mcp_connection_id";
const MCP_CONNECTION_ID_CAMEL_ARG: &str = "__mcpConnectionId";
const MCP_RUN_AS_ARG: &str = "__mcp_run_as";
const MCP_RUN_AS_CAMEL_ARG: &str = "__mcpRunAs";
const MCP_PRINCIPAL_ARG: &str = "__mcp_principal";
const MCP_PRINCIPAL_CAMEL_ARG: &str = "__mcpPrincipal";
const STRICT_TENANT_CONTEXT_ARG: &str = "__strict_tenant_context";
const VERIFIED_TENANT_CONTEXT_ARG: &str = "__verified_tenant_context";

#[derive(Debug, Clone)]
struct McpRunAsRequest {
    connection_id: Option<String>,
    principal: Option<McpPrincipalRef>,
}

#[derive(Debug, Clone)]
struct McpRunAsResolution {
    args: Value,
    requested_tenant_context: TenantContext,
    effective_tenant_context: TenantContext,
    connection_id: String,
    principal: McpPrincipalRef,
    connection_class: Option<String>,
    upstream_account: Option<Value>,
    requested_connection_id: Option<String>,
}

#[derive(Debug, Clone)]
struct McpContextAssertionPreflight {
    decision_id: Option<String>,
    assertion_id: Option<String>,
    grant_id: Option<String>,
    resource: ResourceRef,
    evaluation_reason: String,
}

pub(crate) async fn call_mcp_tool_for_tenant_with_audit(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    args: Value,
    tenant_context: &TenantContext,
) -> Result<ToolResult, String> {
    let verified_context = verified_context_from_tool_args(&args);
    call_mcp_tool_for_tenant_with_trusted_context(
        state,
        server_name,
        tool_name,
        args,
        tenant_context,
        verified_context
            .as_ref()
            .and_then(|context| context.strict_projection.as_ref()),
    )
    .await
}

pub(crate) async fn call_mcp_tool_for_tenant_with_verified_context(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    args: Value,
    tenant_context: &TenantContext,
    verified_context: Option<&VerifiedTenantContext>,
) -> Result<ToolResult, String> {
    call_mcp_tool_for_tenant_with_trusted_context(
        state,
        server_name,
        tool_name,
        args,
        tenant_context,
        verified_context.and_then(|context| context.strict_projection.as_ref()),
    )
    .await
}

async fn call_mcp_tool_for_tenant_with_trusted_context(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    args: Value,
    tenant_context: &TenantContext,
    strict_context: Option<&StrictTenantContext>,
) -> Result<ToolResult, String> {
    let run_as = resolve_mcp_run_as(state, server_name, tool_name, args, tenant_context).await?;
    let context_preflight = enforce_mcp_context_assertion_preflight(
        state,
        server_name,
        tool_name,
        &run_as.effective_tenant_context,
        strict_context,
    )
    .await?;
    let result = state
        .mcp
        .call_tool_for_tenant(
            server_name,
            tool_name,
            run_as.args.clone(),
            &run_as.effective_tenant_context,
        )
        .await;
    if result
        .as_ref()
        .err()
        .is_some_and(|error| mcp_error_is_secret_tenant_mismatch(error))
    {
        append_mcp_secret_tenant_mismatch_audit_event(
            state,
            server_name,
            tool_name,
            &run_as.effective_tenant_context,
        )
        .await;
    }

    append_mcp_tool_execution_audit_event(
        state,
        server_name,
        tool_name,
        &run_as,
        context_preflight.as_ref(),
        &result,
    )
    .await;
    result.map(|mut result| {
        let run_as_payload = run_as.audit_payload();
        if let Some(metadata) = result.metadata.as_object_mut() {
            metadata.insert("mcpRunAs".to_string(), run_as_payload);
            if let Some(preflight) = context_preflight.as_ref() {
                metadata.insert(
                    "contextAssertionPreflight".to_string(),
                    preflight.audit_payload(),
                );
            }
        } else {
            result.metadata = json!({
                "mcpRunAs": run_as_payload,
                "contextAssertionPreflight": context_preflight
                    .as_ref()
                    .map(McpContextAssertionPreflight::audit_payload),
            });
        }
        result
    })
}

fn mcp_error_is_secret_tenant_mismatch(error: &str) -> bool {
    error.contains("ToolDenied { reason: TenantScope }")
        && error.contains("store-backed secret header")
        && error.contains("different tenant context")
}

pub(crate) async fn append_mcp_secret_tenant_mismatch_audit_event(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    tenant_context: &TenantContext,
) {
    let Some(denial) = state
        .mcp
        .secret_tenant_mismatch_audit(server_name, tool_name, tenant_context)
        .await
    else {
        return;
    };
    let _ = crate::audit::append_protected_audit_event(
        state,
        "mcp.secret_tenant_mismatch",
        &denial.tenant_context,
        denial.tenant_context.actor_id.clone(),
        json!({
            "reason": "store_secret_tenant_mismatch",
            "server_name": denial.server_name,
            "tool_name": denial.tool_name,
            "header_names": denial.header_names,
            "tenant_context": denial.tenant_context,
        }),
    )
    .await;
}

async fn resolve_mcp_run_as(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    args: Value,
    tenant_context: &TenantContext,
) -> Result<McpRunAsResolution, String> {
    let request = extract_mcp_run_as_request(&args);
    let effective_tenant_context = match effective_tenant_context_for_run_as(
        tenant_context,
        request.principal.as_ref(),
    ) {
        Ok(context) => context,
        Err(reason) => {
            append_mcp_run_as_denial_audit_event(
                state,
                server_name,
                tool_name,
                tenant_context,
                tenant_context,
                request.connection_id.as_deref(),
                None,
                &reason,
            )
            .await;
            return Err(format!(
                    "ToolDenied {{ reason: McpRunAsPolicy }}: blocked MCP tool `{server_name}.{tool_name}` because {reason}."
                ));
        }
    };
    let expected_connection_id = state
        .mcp
        .connection_id_for_tenant(server_name, &effective_tenant_context);

    if let Some(requested_connection_id) = request.connection_id.as_deref() {
        if requested_connection_id != expected_connection_id {
            let reason = format!(
                "requested connection `{requested_connection_id}` is not owned by the effective tenant/principal"
            );
            append_mcp_run_as_denial_audit_event(
                state,
                server_name,
                tool_name,
                tenant_context,
                &effective_tenant_context,
                request.connection_id.as_deref(),
                Some(&expected_connection_id),
                &reason,
            )
            .await;
            return Err(format!(
                "ToolDenied {{ reason: McpRunAsPolicy }}: blocked MCP tool `{server_name}.{tool_name}` because {reason}."
            ));
        }
    }

    let connections = state.mcp.list_connections().await;
    let connection = connections.get(&expected_connection_id).cloned();
    let expected_principal = McpPrincipalRef::from_tenant_context(&effective_tenant_context);
    if let Some(connection) = connection.as_ref() {
        if connection.tenant_context != effective_tenant_context
            || connection.owner != expected_principal
        {
            let reason = "stored connection identity did not match the effective tenant/principal";
            append_mcp_run_as_denial_audit_event(
                state,
                server_name,
                tool_name,
                tenant_context,
                &effective_tenant_context,
                request.connection_id.as_deref(),
                Some(&expected_connection_id),
                reason,
            )
            .await;
            return Err(format!(
                "ToolDenied {{ reason: McpRunAsPolicy }}: blocked MCP tool `{server_name}.{tool_name}` because {reason}."
            ));
        }
    }
    if let Some(requested_principal) = request.principal.as_ref() {
        let principal_matches = connection
            .as_ref()
            .map(|connection| requested_principal == &connection.owner)
            .unwrap_or_else(|| requested_principal == &expected_principal);
        if !principal_matches {
            let reason = "requested run-as principal did not match the selected connection";
            append_mcp_run_as_denial_audit_event(
                state,
                server_name,
                tool_name,
                tenant_context,
                &effective_tenant_context,
                request.connection_id.as_deref(),
                Some(&expected_connection_id),
                reason,
            )
            .await;
            return Err(format!(
                "ToolDenied {{ reason: McpRunAsPolicy }}: blocked MCP tool `{server_name}.{tool_name}` because {reason}."
            ));
        }
    }

    Ok(McpRunAsResolution {
        args: strip_mcp_run_as_args(args),
        requested_tenant_context: tenant_context.clone(),
        effective_tenant_context,
        connection_id: expected_connection_id,
        principal: expected_principal,
        connection_class: connection.as_ref().and_then(|connection| {
            serde_json::to_value(&connection.connection_class)
                .ok()
                .and_then(|value| value.as_str().map(str::to_string))
        }),
        upstream_account: connection
            .and_then(|connection| serde_json::to_value(connection.upstream_account).ok())
            .filter(|value| !value.is_null()),
        requested_connection_id: request.connection_id,
    })
}

fn effective_tenant_context_for_run_as(
    tenant_context: &TenantContext,
    principal: Option<&McpPrincipalRef>,
) -> Result<TenantContext, String> {
    let Some(principal) = principal else {
        return Ok(tenant_context.clone());
    };
    match principal {
        McpPrincipalRef::HumanActor { actor_id } => {
            if tenant_context.actor_id.as_deref() == Some(actor_id.as_str()) {
                Ok(tenant_context.clone())
            } else {
                Err(format!(
                    "human actor `{actor_id}` does not match the request tenant actor"
                ))
            }
        }
        McpPrincipalRef::ServicePrincipal { .. } => {
            if tenant_context.actor_id.is_some() {
                return Err(
                    "service-principal MCP run-as requires a server-side connection grant and cannot be selected from an actor-scoped request"
                        .to_string(),
                );
            }
            let mut service_tenant = tenant_context.clone();
            service_tenant.actor_id = None;
            Ok(service_tenant)
        }
        McpPrincipalRef::LocalImplicit => {
            if tenant_context.is_local_implicit() {
                Ok(tenant_context.clone())
            } else {
                Err(
                    "local-implicit MCP connections cannot be selected from explicit tenants"
                        .to_string(),
                )
            }
        }
        McpPrincipalRef::AutomationPrincipal { .. } | McpPrincipalRef::SharedConnection { .. } => {
            Err(
                "the selected delegated MCP principal is not executable by the current bridge"
                    .to_string(),
            )
        }
    }
}

fn extract_mcp_run_as_request(args: &Value) -> McpRunAsRequest {
    let Some(object) = args.as_object() else {
        return McpRunAsRequest {
            connection_id: None,
            principal: None,
        };
    };
    let run_as = object
        .get(MCP_RUN_AS_ARG)
        .or_else(|| object.get(MCP_RUN_AS_CAMEL_ARG));
    let connection_id = object
        .get(MCP_CONNECTION_ID_ARG)
        .or_else(|| object.get(MCP_CONNECTION_ID_CAMEL_ARG))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| run_as.and_then(connection_id_from_run_as_value));
    let principal = object
        .get(MCP_PRINCIPAL_ARG)
        .or_else(|| object.get(MCP_PRINCIPAL_CAMEL_ARG))
        .and_then(parse_mcp_principal_ref)
        .or_else(|| run_as.and_then(principal_from_run_as_value));
    McpRunAsRequest {
        connection_id,
        principal,
    }
}

fn connection_id_from_run_as_value(value: &Value) -> Option<String> {
    value
        .get("connection_id")
        .or_else(|| value.get("connectionId"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn principal_from_run_as_value(value: &Value) -> Option<McpPrincipalRef> {
    value
        .get("principal")
        .and_then(parse_mcp_principal_ref)
        .or_else(|| parse_mcp_principal_ref(value))
}

fn parse_mcp_principal_ref(value: &Value) -> Option<McpPrincipalRef> {
    serde_json::from_value::<McpPrincipalRef>(value.clone()).ok()
}

fn verified_context_from_tool_args(args: &Value) -> Option<VerifiedTenantContext> {
    args.get(VERIFIED_TENANT_CONTEXT_ARG)
        .cloned()
        .and_then(|value| serde_json::from_value::<VerifiedTenantContext>(value).ok())
}

fn strip_mcp_run_as_args(args: Value) -> Value {
    let Value::Object(mut object) = args else {
        return args;
    };
    for key in [
        MCP_CONNECTION_ID_ARG,
        MCP_CONNECTION_ID_CAMEL_ARG,
        MCP_RUN_AS_ARG,
        MCP_RUN_AS_CAMEL_ARG,
        MCP_PRINCIPAL_ARG,
        MCP_PRINCIPAL_CAMEL_ARG,
        STRICT_TENANT_CONTEXT_ARG,
        VERIFIED_TENANT_CONTEXT_ARG,
    ] {
        object.remove(key);
    }
    Value::Object(object)
}

async fn enforce_mcp_context_assertion_preflight(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    effective_tenant_context: &TenantContext,
    strict_context: Option<&StrictTenantContext>,
) -> Result<Option<McpContextAssertionPreflight>, String> {
    if effective_tenant_context.is_local_implicit() && strict_context.is_none() {
        return Ok(None);
    }

    let resource = mcp_tool_resource_ref(effective_tenant_context, server_name, tool_name);
    let Some(strict_context) = strict_context else {
        let reason = "missing_verified_tenant_context";
        let decision_id = record_mcp_context_assertion_decision(
            state,
            effective_tenant_context,
            server_name,
            tool_name,
            &resource,
            None,
            None,
            PolicyDecisionEffect::Deny,
            reason,
            None,
        )
        .await;
        append_mcp_context_assertion_denial_audit_event(
            state,
            effective_tenant_context,
            server_name,
            tool_name,
            &resource,
            decision_id.as_deref(),
            reason,
            None,
            None,
        )
        .await;
        return Err(format!(
            "ToolDenied {{ reason: ContextAssertion }}: blocked MCP tool `{server_name}.{tool_name}` because a verified tenant context assertion is required."
        ));
    };

    if !tenant_context_matches(&strict_context.tenant_context, effective_tenant_context) {
        let reason = "verified_tenant_context_mismatch";
        let decision_id = record_mcp_context_assertion_decision(
            state,
            effective_tenant_context,
            server_name,
            tool_name,
            &resource,
            Some(strict_context),
            None,
            PolicyDecisionEffect::Deny,
            reason,
            None,
        )
        .await;
        append_mcp_context_assertion_denial_audit_event(
            state,
            effective_tenant_context,
            server_name,
            tool_name,
            &resource,
            decision_id.as_deref(),
            reason,
            Some(strict_context),
            None,
        )
        .await;
        return Err(format!(
            "ToolDenied {{ reason: ContextAssertion }}: blocked MCP tool `{server_name}.{tool_name}` because the verified tenant context does not match the effective tenant."
        ));
    }

    let evaluation = strict_context.evaluate_access(
        &resource,
        AccessPermission::Execute,
        DataClass::Internal,
        now_ms(),
    );
    let effect = if evaluation.decision == AccessDecision::Allow {
        PolicyDecisionEffect::Allow
    } else {
        PolicyDecisionEffect::Deny
    };
    let decision_id = record_mcp_context_assertion_decision(
        state,
        effective_tenant_context,
        server_name,
        tool_name,
        &resource,
        Some(strict_context),
        Some(&evaluation),
        effect,
        &evaluation.reason,
        evaluation.grant_id.clone(),
    )
    .await;

    if evaluation.decision != AccessDecision::Allow {
        append_mcp_context_assertion_denial_audit_event(
            state,
            effective_tenant_context,
            server_name,
            tool_name,
            &resource,
            decision_id.as_deref(),
            &evaluation.reason,
            Some(strict_context),
            evaluation.grant_id.as_deref(),
        )
        .await;
        return Err(format!(
            "ToolDenied {{ reason: ContextAssertion }}: blocked MCP tool `{server_name}.{tool_name}` because context assertion evaluation returned `{}`.",
            evaluation.reason
        ));
    }

    Ok(Some(McpContextAssertionPreflight {
        decision_id,
        assertion_id: Some(strict_context.assertion.assertion_id.clone()),
        grant_id: evaluation.grant_id,
        resource,
        evaluation_reason: evaluation.reason,
    }))
}

fn tenant_context_matches(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
        && left.actor_id == right.actor_id
}

fn mcp_tool_resource_ref(
    tenant_context: &TenantContext,
    server_name: &str,
    tool_name: &str,
) -> ResourceRef {
    ResourceRef::new(
        tenant_context.org_id.clone(),
        tenant_context.workspace_id.clone(),
        ResourceKind::McpTool,
        format!(
            "mcp.{}.{}",
            super::mcp::mcp_namespace_segment(server_name),
            super::mcp::mcp_namespace_segment(tool_name)
        ),
    )
    .with_parent_path(vec![ResourcePathSegment::new(
        ResourceKind::McpServer,
        server_name.to_string(),
    )])
}

#[allow(clippy::too_many_arguments)]
async fn record_mcp_context_assertion_decision(
    state: &AppState,
    tenant_context: &TenantContext,
    server_name: &str,
    tool_name: &str,
    resource: &ResourceRef,
    strict_context: Option<&StrictTenantContext>,
    evaluation: Option<&GrantEvaluation>,
    effect: PolicyDecisionEffect,
    reason: &str,
    grant_id: Option<String>,
) -> Option<String> {
    let decision_id = format!("policy_decision_{}", uuid::Uuid::new_v4().simple());
    let actor_id = strict_context
        .and_then(|context| context.principal.tenant_actor_id.clone())
        .or_else(|| tenant_context.actor_id.clone())
        .or_else(|| strict_context.map(|context| context.principal.id.clone()));
    let record = PolicyDecisionRecord {
        decision_id: decision_id.clone(),
        tenant_context: tenant_context.clone(),
        actor_id,
        session_id: None,
        message_id: None,
        run_id: None,
        automation_id: None,
        node_id: None,
        tool: Some(format!("mcp.{server_name}.{tool_name}")),
        resource: Some(resource.clone()),
        data_classes: vec![DataClass::Internal],
        risk_tier: None,
        decision: effect,
        reason_code: reason.to_string(),
        reason: format!("MCP context assertion preflight: {reason}"),
        policy_id: Some("mcp_context_assertion_preflight".to_string()),
        grant_id,
        approval_id: None,
        audit_event_id: None,
        created_at_ms: now_ms(),
        metadata: json!({
            "context_assertion": strict_context.map(|context| {
                json!({
                    "assertion_id": context.assertion.assertion_id,
                    "issuer": context.assertion.issuer,
                    "audience": context.assertion.audience,
                    "expires_at_ms": context.assertion.expires_at_ms,
                    "principal": context.principal,
                })
            }),
            "evaluation": evaluation,
            "permission": AccessPermission::Execute,
            "data_class": DataClass::Internal,
            "server_name": server_name,
            "tool_name": tool_name,
        }),
    };
    match state.record_policy_decision(record).await {
        Ok(record) => Some(record.decision_id),
        Err(error) => {
            tracing::warn!("failed to record MCP context assertion decision: {error:?}");
            None
        }
    }
}

async fn append_mcp_context_assertion_denial_audit_event(
    state: &AppState,
    tenant_context: &TenantContext,
    server_name: &str,
    tool_name: &str,
    resource: &ResourceRef,
    decision_id: Option<&str>,
    reason: &str,
    strict_context: Option<&StrictTenantContext>,
    grant_id: Option<&str>,
) {
    let actor_id = strict_context
        .and_then(|context| context.principal.tenant_actor_id.clone())
        .or_else(|| tenant_context.actor_id.clone())
        .or_else(|| strict_context.map(|context| context.principal.id.clone()));
    let _ = crate::audit::append_protected_audit_event(
        state,
        "mcp.context_assertion_denied",
        tenant_context,
        actor_id,
        json!({
            "decision_id": decision_id,
            "reason": reason,
            "server_name": server_name,
            "tool_name": tool_name,
            "resource": resource,
            "assertion_id": strict_context.map(|context| context.assertion.assertion_id.as_str()),
            "grant_id": grant_id,
            "tenant_context": tenant_context,
            "created_at_ms": now_ms(),
        }),
    )
    .await;
}

async fn append_mcp_run_as_denial_audit_event(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    requested_tenant_context: &TenantContext,
    effective_tenant_context: &TenantContext,
    requested_connection_id: Option<&str>,
    expected_connection_id: Option<&str>,
    reason: &str,
) {
    let _ = crate::audit::append_protected_audit_event(
        state,
        "mcp.run_as_denied",
        effective_tenant_context,
        requested_tenant_context.actor_id.clone(),
        json!({
            "reason": reason,
            "server_name": server_name,
            "tool_name": tool_name,
            "requested_connection_id": requested_connection_id,
            "expected_connection_id": expected_connection_id,
            "requested_tenant_context": requested_tenant_context,
            "effective_tenant_context": effective_tenant_context,
            "created_at_ms": now_ms(),
        }),
    )
    .await;
}

async fn append_mcp_tool_execution_audit_event(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    run_as: &McpRunAsResolution,
    context_preflight: Option<&McpContextAssertionPreflight>,
    result: &Result<ToolResult, String>,
) {
    let _ = crate::audit::append_protected_audit_event(
        state,
        "mcp.tool.execution",
        &run_as.effective_tenant_context,
        run_as.requested_tenant_context.actor_id.clone(),
        json!({
            "status": if result.is_ok() { "completed" } else { "failed" },
            "server_name": server_name,
            "tool_name": tool_name,
            "connection_id": run_as.connection_id,
            "requested_connection_id": run_as.requested_connection_id,
            "principal": run_as.principal,
            "connection_class": run_as.connection_class,
            "upstream_account": run_as.upstream_account,
            "requested_tenant_context": run_as.requested_tenant_context,
            "effective_tenant_context": run_as.effective_tenant_context,
            "context_assertion_preflight": context_preflight.map(McpContextAssertionPreflight::audit_payload),
            "error": result.as_ref().err().map(|error| error.as_str()),
        }),
    )
    .await;
}

impl McpRunAsResolution {
    fn audit_payload(&self) -> Value {
        json!({
            "connectionId": self.connection_id,
            "requestedConnectionId": self.requested_connection_id,
            "principal": self.principal,
            "connectionClass": self.connection_class,
            "upstreamAccount": self.upstream_account,
            "requestedTenantContext": self.requested_tenant_context,
            "effectiveTenantContext": self.effective_tenant_context,
        })
    }
}

impl McpContextAssertionPreflight {
    fn audit_payload(&self) -> Value {
        json!({
            "policyDecisionId": self.decision_id,
            "assertionId": self.assertion_id,
            "grantId": self.grant_id,
            "resource": self.resource,
            "permission": AccessPermission::Execute,
            "dataClass": DataClass::Internal,
            "reason": self.evaluation_reason,
        })
    }
}
