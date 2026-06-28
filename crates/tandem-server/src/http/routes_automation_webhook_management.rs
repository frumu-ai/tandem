use axum::extract::{Extension, Path, Query, State};
use axum::http::header::HOST;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tandem_types::{
    AccessDecision, AccessPermission, DataClass, PrincipalRef, RequestPrincipal, ResourceScope,
    TenantContext, ToolRiskTier, VerifiedTenantContext,
};

use crate::app::state::{AutomationWebhookTriggerCreateInput, AutomationWebhookTriggerUpdateInput};
use crate::automation_v2::types::{
    automation_webhook_provider_event_id_headers, normalize_automation_webhook_provider,
    AutomationV2Spec, AutomationWebhookDeliveryRecord, AutomationWebhookDeliveryStatus,
    AutomationWebhookTriggerRecord,
};
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/automations/v2/{id}/webhook-triggers",
            get(list_webhook_triggers).post(create_webhook_trigger),
        )
        .route(
            "/automations/v2/{id}/webhook-triggers/{trigger_id}",
            get(get_webhook_trigger)
                .patch(update_webhook_trigger)
                .delete(delete_webhook_trigger),
        )
        .route(
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/disable",
            post(disable_webhook_trigger),
        )
        .route(
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/rotate-secret",
            post(rotate_webhook_secret),
        )
        .route(
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/deliveries",
            get(list_webhook_deliveries),
        )
        .route(
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/deliveries/{delivery_id}",
            get(get_webhook_delivery),
        )
}

#[derive(Default, Deserialize)]
struct WebhookTriggerCreateRequest {
    #[serde(default)]
    name: Option<String>,
    provider: String,
    #[serde(default)]
    provider_event_kind: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    owning_org_unit_id: Option<String>,
    #[serde(default)]
    resource_scope: Option<ResourceScope>,
    #[serde(default)]
    default_data_class: Option<DataClass>,
    #[serde(default)]
    default_risk_tier: Option<ToolRiskTier>,
}

fn nullable_string_patch<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

fn nullable_risk_tier_patch<'de, D>(
    deserializer: D,
) -> Result<Option<Option<ToolRiskTier>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<ToolRiskTier>::deserialize(deserializer).map(Some)
}

#[derive(Default, Deserialize)]
struct WebhookTriggerUpdateRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default, deserialize_with = "nullable_string_patch")]
    provider_event_kind: Option<Option<String>>,
    #[serde(default)]
    default_data_class: Option<DataClass>,
    #[serde(default, deserialize_with = "nullable_risk_tier_patch")]
    default_risk_tier: Option<Option<ToolRiskTier>>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Default, Deserialize)]
struct DeliveryListQuery {
    #[serde(default)]
    limit: Option<usize>,
}

fn error_response(
    status: StatusCode,
    code: &'static str,
    error: impl Into<String>,
) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({
            "error": error.into(),
            "code": code,
        })),
    )
}

fn automation_not_found(id: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "Automation not found",
            "code": "AUTOMATION_V2_NOT_FOUND",
            "automationID": id,
        })),
    )
}

fn webhook_trigger_not_found() -> (StatusCode, Json<Value>) {
    error_response(
        StatusCode::NOT_FOUND,
        "AUTOMATION_WEBHOOK_TRIGGER_NOT_FOUND",
        "Webhook trigger not found",
    )
}

fn access_denied() -> (StatusCode, Json<Value>) {
    error_response(
        StatusCode::FORBIDDEN,
        "AUTOMATION_WEBHOOK_ACCESS_DENIED",
        "Webhook trigger access denied",
    )
}

fn hosted_context_admin(verified: Option<&VerifiedTenantContext>) -> bool {
    let Some(verified) = verified else {
        return false;
    };
    verified.roles.iter().any(|role| {
        matches!(
            role.as_str(),
            "owner"
                | "admin"
                | "hosted:owner"
                | "hosted:admin"
                | "enterprise:admin"
                | "workspace:admin"
                | "organization:admin"
        )
    }) || verified.capabilities.iter().any(|capability| {
        matches!(
            capability.as_str(),
            "hosted.owner" | "hosted.admin" | "automation.write" | "automation.share"
        )
    })
}

fn hosted_context_actor_id(verified: Option<&VerifiedTenantContext>) -> Option<&str> {
    verified
        .map(|context| context.human_actor.actor_id.trim())
        .filter(|actor_id| !actor_id.is_empty())
}

fn automation_access_metadata(
    automation: &AutomationV2Spec,
) -> Option<&serde_json::Map<String, Value>> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("resource_access"))
        .and_then(Value::as_object)
}

fn automation_access_visibility(automation: &AutomationV2Spec) -> Option<&str> {
    automation_access_metadata(automation)
        .and_then(|metadata| metadata.get("visibility"))
        .and_then(Value::as_str)
}

fn automation_access_owner(automation: &AutomationV2Spec) -> Option<&str> {
    automation_access_metadata(automation)
        .and_then(|metadata| metadata.get("owner_principal"))
        .and_then(Value::as_object)
        .and_then(|owner| owner.get("id"))
        .and_then(Value::as_str)
}

fn automation_access_audiences(automation: &AutomationV2Spec) -> Vec<String> {
    automation_access_metadata(automation)
        .and_then(|metadata| metadata.get("audience_principals"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn automation_visible_to_context(
    automation: &AutomationV2Spec,
    verified: Option<&VerifiedTenantContext>,
) -> bool {
    if verified.is_none() || automation_access_metadata(automation).is_none() {
        return true;
    }
    if hosted_context_admin(verified) {
        return true;
    }
    let Some(actor_id) = hosted_context_actor_id(verified) else {
        return false;
    };
    if automation_access_owner(automation) == Some(actor_id) {
        return true;
    }
    match automation_access_visibility(automation).unwrap_or("private") {
        "org" => true,
        "group" => {
            let audience = automation_access_audiences(automation);
            let groups = verified
                .map(|context| context.org_units.as_slice())
                .unwrap_or(&[]);
            groups
                .iter()
                .any(|group| audience.iter().any(|entry| entry == group))
        }
        _ => false,
    }
}

fn automation_owner_or_admin(
    automation: &AutomationV2Spec,
    verified: Option<&VerifiedTenantContext>,
) -> bool {
    if verified.is_none() || automation_access_metadata(automation).is_none() {
        return true;
    }
    let actor_id = hosted_context_actor_id(verified);
    hosted_context_admin(verified) || actor_id == automation_access_owner(automation)
}

async fn load_automation_for_read(
    state: &AppState,
    tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    id: &str,
) -> Result<AutomationV2Spec, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(id).await else {
        return Err(automation_not_found(id));
    };
    super::ensure_same_tenant(tenant_context, &automation.tenant_context())
        .map_err(|_| automation_not_found(id))?;
    if !automation_visible_to_context(&automation, verified) {
        return Err(automation_not_found(id));
    }
    Ok(automation)
}

async fn load_automation_for_mutation(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified: Option<&VerifiedTenantContext>,
    headers: &HeaderMap,
    id: &str,
    delete_intent: bool,
) -> Result<AutomationV2Spec, (StatusCode, Json<Value>)> {
    let automation = load_automation_for_read(state, tenant_context, verified, id).await?;
    if !automation_owner_or_admin(&automation, verified) {
        return Err(access_denied());
    }
    let actor =
        super::governance::resolve_governance_actor(headers, tenant_context, request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    super::governance::enforce_mutation_or_audit(
        state,
        tenant_context,
        id,
        &actor,
        state.can_mutate_automation(id, &actor, delete_intent).await,
    )
    .await?;
    Ok(automation)
}

fn strict_scope_allows(
    verified: &VerifiedTenantContext,
    scope: &ResourceScope,
    permission: AccessPermission,
    data_class: DataClass,
) -> bool {
    let Some(strict) = verified.strict_projection.as_ref() else {
        return false;
    };
    let now_ms = crate::now_ms();
    let requested = strict.evaluate_access(&scope.root, permission, data_class, now_ms);
    if requested.decision == AccessDecision::Allow {
        return true;
    }
    if permission == AccessPermission::Admin {
        return false;
    }
    strict
        .evaluate_access(&scope.root, AccessPermission::Admin, data_class, now_ms)
        .decision
        == AccessDecision::Allow
}

fn trigger_scope_allowed(
    trigger: &AutomationWebhookTriggerRecord,
    verified: Option<&VerifiedTenantContext>,
    permission: AccessPermission,
) -> bool {
    let Some(verified) = verified else {
        return true;
    };
    if hosted_context_admin(Some(verified)) {
        return true;
    }
    if let Some(org_unit_id) = trigger
        .owning_org_unit_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !verified.org_units.iter().any(|unit| unit == org_unit_id) {
            return false;
        }
    }
    if let Some(scope) = trigger.resource_scope.as_ref() {
        return strict_scope_allows(verified, scope, permission, trigger.default_data_class);
    }
    true
}

fn requested_scope_allowed(
    owning_org_unit_id: Option<&str>,
    resource_scope: Option<&ResourceScope>,
    data_class: DataClass,
    verified: Option<&VerifiedTenantContext>,
) -> bool {
    let Some(verified) = verified else {
        return true;
    };
    if hosted_context_admin(Some(verified)) {
        return true;
    }
    if let Some(org_unit_id) = owning_org_unit_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if !verified.org_units.iter().any(|unit| unit == org_unit_id) {
            return false;
        }
    }
    if let Some(scope) = resource_scope {
        return strict_scope_allows(verified, scope, AccessPermission::Admin, data_class)
            || strict_scope_allows(verified, scope, AccessPermission::Edit, data_class);
    }
    true
}

async fn load_trigger_for_read(
    state: &AppState,
    tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    automation_id: &str,
    trigger_id: &str,
) -> Result<AutomationWebhookTriggerRecord, (StatusCode, Json<Value>)> {
    let Some(trigger) = state
        .get_automation_webhook_trigger(tenant_context, trigger_id)
        .await
    else {
        return Err(webhook_trigger_not_found());
    };
    if trigger.automation_id != automation_id {
        return Err(webhook_trigger_not_found());
    }
    if !trigger_scope_allowed(&trigger, verified, AccessPermission::View) {
        return Err(webhook_trigger_not_found());
    }
    Ok(trigger)
}

async fn load_trigger_for_mutation(
    state: &AppState,
    tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    automation_id: &str,
    trigger_id: &str,
) -> Result<AutomationWebhookTriggerRecord, (StatusCode, Json<Value>)> {
    let trigger =
        load_trigger_for_read(state, tenant_context, verified, automation_id, trigger_id).await?;
    if !trigger_scope_allowed(&trigger, verified, AccessPermission::Admin) {
        return Err(access_denied());
    }
    Ok(trigger)
}

fn trigger_display_name(trigger: &AutomationWebhookTriggerRecord) -> String {
    let name = trigger.name.trim();
    if name.is_empty() {
        trigger.provider.clone()
    } else {
        name.to_string()
    }
}

fn callback_path(trigger: &AutomationWebhookTriggerRecord) -> String {
    format!("/webhooks/automations/{}", trigger.public_path_token)
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn callback_url(headers: &HeaderMap, trigger: &AutomationWebhookTriggerRecord) -> String {
    let path = callback_path(trigger);
    let host = header_string(headers, "x-forwarded-host").or_else(|| {
        headers
            .get(HOST)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    });
    let Some(host) = host else {
        return path;
    };
    let scheme = header_string(headers, "x-forwarded-proto").unwrap_or_else(|| "http".to_string());
    format!("{}://{}{}", scheme, host, path)
}

fn delivery_status_key(status: &AutomationWebhookDeliveryStatus) -> &'static str {
    match status {
        AutomationWebhookDeliveryStatus::Received => "received",
        AutomationWebhookDeliveryStatus::Accepted => "accepted",
        AutomationWebhookDeliveryStatus::Rejected => "rejected",
        AutomationWebhookDeliveryStatus::Duplicate => "duplicate",
        AutomationWebhookDeliveryStatus::Disabled => "disabled",
        AutomationWebhookDeliveryStatus::Failed => "failed",
    }
}

fn delivery_counts(deliveries: &[AutomationWebhookDeliveryRecord]) -> Value {
    let mut received = 0usize;
    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut duplicate = 0usize;
    let mut disabled = 0usize;
    let mut failed = 0usize;
    for delivery in deliveries {
        match delivery.status {
            AutomationWebhookDeliveryStatus::Received => received += 1,
            AutomationWebhookDeliveryStatus::Accepted => accepted += 1,
            AutomationWebhookDeliveryStatus::Rejected => rejected += 1,
            AutomationWebhookDeliveryStatus::Duplicate => duplicate += 1,
            AutomationWebhookDeliveryStatus::Disabled => disabled += 1,
            AutomationWebhookDeliveryStatus::Failed => failed += 1,
        }
    }
    json!({
        "total": deliveries.len(),
        "received": received,
        "accepted": accepted,
        "rejected": rejected,
        "duplicate": duplicate,
        "disabled": disabled,
        "failed": failed,
    })
}

fn provider_metadata(trigger: &AutomationWebhookTriggerRecord) -> Value {
    let canonical_provider = normalize_automation_webhook_provider(&trigger.provider)
        .unwrap_or_else(|| "generic".to_string());
    let event_id_headers = automation_webhook_provider_event_id_headers(&canonical_provider);
    json!({
        "canonical_provider": canonical_provider.as_str(),
        "canonicalProvider": canonical_provider.as_str(),
        "provider_event_kind": trigger.provider_event_kind,
        "providerEventKind": trigger.provider_event_kind,
        "event_id_headers": event_id_headers,
        "eventIdHeaders": event_id_headers,
        "verification": {
            "signature_scheme": trigger.signature_scheme,
            "signatureScheme": trigger.signature_scheme,
            "provider_specific": false,
            "providerSpecific": false,
        },
        "polling": {
            "supported": false,
            "reconciliation_supported": false,
            "reconciliationSupported": false,
        },
    })
}

fn trigger_value(
    trigger: &AutomationWebhookTriggerRecord,
    deliveries: &[AutomationWebhookDeliveryRecord],
    headers: &HeaderMap,
) -> Value {
    json!({
        "trigger_id": trigger.trigger_id,
        "triggerID": trigger.trigger_id,
        "automation_id": trigger.automation_id,
        "automationID": trigger.automation_id,
        "name": trigger_display_name(trigger),
        "provider": trigger.provider,
        "provider_event_kind": trigger.provider_event_kind,
        "providerEventKind": trigger.provider_event_kind,
        "provider_metadata": provider_metadata(trigger),
        "providerMetadata": provider_metadata(trigger),
        "enabled": trigger.enabled,
        "callback_path": callback_path(trigger),
        "callbackPath": callback_path(trigger),
        "callback_url": callback_url(headers, trigger),
        "callbackUrl": callback_url(headers, trigger),
        "tenant_label": format!("{} / {}", trigger.tenant_context.org_id, trigger.tenant_context.workspace_id),
        "tenantLabel": format!("{} / {}", trigger.tenant_context.org_id, trigger.tenant_context.workspace_id),
        "owning_org_unit_id": trigger.owning_org_unit_id,
        "owningOrgUnitId": trigger.owning_org_unit_id,
        "resource_scope": trigger.resource_scope,
        "resourceScope": trigger.resource_scope,
        "default_data_class": trigger.default_data_class,
        "defaultDataClass": trigger.default_data_class,
        "default_risk_tier": trigger.default_risk_tier,
        "defaultRiskTier": trigger.default_risk_tier,
        "signature_scheme": trigger.signature_scheme,
        "signatureScheme": trigger.signature_scheme,
        "secret_status": {
            "configured": true,
            "secret_version": trigger.secret.secret_version,
            "secretVersion": trigger.secret.secret_version,
            "created_at_ms": trigger.secret.created_at_ms,
            "createdAtMs": trigger.secret.created_at_ms,
            "rotated_at_ms": trigger.secret.rotated_at_ms,
            "rotatedAtMs": trigger.secret.rotated_at_ms,
            "rotated_by": trigger.secret.rotated_by,
            "rotatedBy": trigger.secret.rotated_by,
        },
        "created_at_ms": trigger.created_at_ms,
        "createdAtMs": trigger.created_at_ms,
        "updated_at_ms": trigger.updated_at_ms,
        "updatedAtMs": trigger.updated_at_ms,
        "last_received_at_ms": trigger.last_received_at_ms,
        "lastReceivedAtMs": trigger.last_received_at_ms,
        "last_accepted_at_ms": trigger.last_accepted_at_ms,
        "lastAcceptedAtMs": trigger.last_accepted_at_ms,
        "last_rejected_at_ms": trigger.last_rejected_at_ms,
        "lastRejectedAtMs": trigger.last_rejected_at_ms,
        "delivery_counts": delivery_counts(deliveries),
        "deliveryCounts": delivery_counts(deliveries),
    })
}

fn delivery_value(delivery: &AutomationWebhookDeliveryRecord) -> Value {
    json!({
        "delivery_id": delivery.delivery_id,
        "deliveryID": delivery.delivery_id,
        "trigger_id": delivery.trigger_id,
        "triggerID": delivery.trigger_id,
        "automation_id": delivery.automation_id,
        "automationID": delivery.automation_id,
        "provider_event_id": delivery.provider_event_id,
        "providerEventID": delivery.provider_event_id,
        "body_digest": delivery.body_digest,
        "bodyDigest": delivery.body_digest,
        "signature_scheme": delivery.signature_scheme,
        "signatureScheme": delivery.signature_scheme,
        "verification_result_code": delivery.verification_result_code,
        "verificationResultCode": delivery.verification_result_code,
        "status": delivery_status_key(&delivery.status),
        "rejection_reason_code": delivery.rejection_reason_code,
        "rejectionReasonCode": delivery.rejection_reason_code,
        "queued_run_id": delivery.queued_run_id,
        "queuedRunID": delivery.queued_run_id,
        "queued_run_path": delivery.queued_run_id.as_ref().map(|run_id| format!("/automations/v2/runs/{run_id}")),
        "queuedRunPath": delivery.queued_run_id.as_ref().map(|run_id| format!("/automations/v2/runs/{run_id}")),
        "received_at_ms": delivery.received_at_ms,
        "receivedAtMs": delivery.received_at_ms,
        "accepted_at_ms": delivery.accepted_at_ms,
        "acceptedAtMs": delivery.accepted_at_ms,
        "rejected_at_ms": delivery.rejected_at_ms,
        "rejectedAtMs": delivery.rejected_at_ms,
        "sanitized_preview": delivery.sanitized_preview,
        "sanitizedPreview": delivery.sanitized_preview,
        "audit_event_id": delivery.audit_event_id,
        "auditEventID": delivery.audit_event_id,
    })
}

fn actor_id_for_records(
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified: Option<&VerifiedTenantContext>,
) -> Option<String> {
    hosted_context_actor_id(verified)
        .map(ToOwned::to_owned)
        .or_else(|| request_principal.actor_id.clone())
        .or_else(|| tenant_context.actor_id.clone())
}

fn audit_actor(
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified: Option<&VerifiedTenantContext>,
) -> Option<String> {
    actor_id_for_records(tenant_context, request_principal, verified)
        .or_else(|| Some(request_principal.source.clone()))
}

async fn append_webhook_audit(
    state: &AppState,
    event_type: &'static str,
    tenant_context: &TenantContext,
    actor: Option<String>,
    details: Value,
) {
    let _ = crate::audit::append_protected_audit_event(
        state,
        event_type,
        tenant_context,
        actor,
        details,
    )
    .await;
}

async fn list_webhook_triggers(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let _automation = load_automation_for_read(&state, &tenant_context, verified, &id).await?;
    let mut triggers = state
        .list_automation_webhook_triggers_for_automation(&tenant_context, &id)
        .await
        .into_iter()
        .filter(|trigger| trigger_scope_allowed(trigger, verified, AccessPermission::View))
        .collect::<Vec<_>>();
    triggers.sort_by(|left, right| right.created_at_ms.cmp(&left.created_at_ms));
    let rows = futures::future::join_all(triggers.iter().map(|trigger| {
        let state = state.clone();
        let tenant_context = tenant_context.clone();
        let headers = headers.clone();
        async move {
            let deliveries = state
                .list_automation_webhook_deliveries_for_trigger(
                    &tenant_context,
                    &trigger.trigger_id,
                )
                .await;
            trigger_value(trigger, &deliveries, &headers)
        }
    }))
    .await;
    Ok(Json(json!({
        "triggers": rows,
        "count": rows.len(),
    })))
}

async fn create_webhook_trigger(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<WebhookTriggerCreateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let _automation = load_automation_for_mutation(
        &state,
        &tenant_context,
        &request_principal,
        verified,
        &headers,
        &id,
        false,
    )
    .await?;
    let default_data_class = input.default_data_class.unwrap_or(DataClass::Internal);
    if !requested_scope_allowed(
        input.owning_org_unit_id.as_deref(),
        input.resource_scope.as_ref(),
        default_data_class,
        verified,
    ) {
        return Err(access_denied());
    }
    let actor_id = actor_id_for_records(&tenant_context, &request_principal, verified);
    let result = state
        .create_automation_webhook_trigger(AutomationWebhookTriggerCreateInput {
            automation_id: id.clone(),
            tenant_context: tenant_context.clone(),
            owner_principal: actor_id.clone().map(PrincipalRef::human_user),
            created_by: actor_id.clone(),
            owning_org_unit_id: input
                .owning_org_unit_id
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            resource_scope: input.resource_scope,
            default_data_class,
            default_risk_tier: input.default_risk_tier,
            name: input.name,
            provider: input.provider,
            provider_event_kind: input
                .provider_event_kind
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            enabled: input.enabled.unwrap_or(true),
        })
        .await
        .map_err(|error| {
            error_response(
                StatusCode::BAD_REQUEST,
                "AUTOMATION_WEBHOOK_CREATE_FAILED",
                error.to_string(),
            )
        })?;
    append_webhook_audit(
        &state,
        "automation.webhook_trigger.created",
        &tenant_context,
        audit_actor(&tenant_context, &request_principal, verified),
        json!({
            "automationID": id,
            "triggerID": result.trigger.trigger_id,
            "provider": result.trigger.provider,
            "providerEventKind": result.trigger.provider_event_kind,
        }),
    )
    .await;
    Ok(Json(json!({
        "trigger": trigger_value(&result.trigger, &[], &headers),
        "new_secret": result.secret,
        "newSecret": result.secret,
        "secret_one_time": true,
        "secretOneTime": true,
    })))
}

async fn get_webhook_trigger(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((id, trigger_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let _automation = load_automation_for_read(&state, &tenant_context, verified, &id).await?;
    let trigger =
        load_trigger_for_read(&state, &tenant_context, verified, &id, &trigger_id).await?;
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_context, &trigger_id)
        .await;
    Ok(Json(json!({
        "trigger": trigger_value(&trigger, &deliveries, &headers),
    })))
}

async fn update_webhook_trigger(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((id, trigger_id)): Path<(String, String)>,
    Json(input): Json<WebhookTriggerUpdateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let _automation = load_automation_for_mutation(
        &state,
        &tenant_context,
        &request_principal,
        verified,
        &headers,
        &id,
        false,
    )
    .await?;
    let _trigger =
        load_trigger_for_mutation(&state, &tenant_context, verified, &id, &trigger_id).await?;
    let actor_id = actor_id_for_records(&tenant_context, &request_principal, verified);
    let updated = state
        .update_automation_webhook_trigger(
            &tenant_context,
            &id,
            &trigger_id,
            AutomationWebhookTriggerUpdateInput {
                name: input.name,
                provider: input.provider,
                provider_event_kind: input.provider_event_kind,
                default_data_class: input.default_data_class,
                default_risk_tier: input.default_risk_tier,
                enabled: input.enabled,
            },
            actor_id,
        )
        .await
        .map_err(|error| {
            error_response(
                StatusCode::BAD_REQUEST,
                "AUTOMATION_WEBHOOK_UPDATE_FAILED",
                error.to_string(),
            )
        })?;
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_context, &trigger_id)
        .await;
    append_webhook_audit(
        &state,
        "automation.webhook_trigger.updated",
        &tenant_context,
        audit_actor(&tenant_context, &request_principal, verified),
        json!({
            "automationID": id,
            "triggerID": trigger_id,
        }),
    )
    .await;
    Ok(Json(json!({
        "trigger": trigger_value(&updated, &deliveries, &headers),
    })))
}

async fn disable_webhook_trigger(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((id, trigger_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let _automation = load_automation_for_mutation(
        &state,
        &tenant_context,
        &request_principal,
        verified,
        &headers,
        &id,
        false,
    )
    .await?;
    let _trigger =
        load_trigger_for_mutation(&state, &tenant_context, verified, &id, &trigger_id).await?;
    let actor_id = actor_id_for_records(&tenant_context, &request_principal, verified);
    let updated = state
        .disable_automation_webhook_trigger(&tenant_context, &trigger_id, actor_id)
        .await
        .map_err(|error| {
            error_response(
                StatusCode::BAD_REQUEST,
                "AUTOMATION_WEBHOOK_DISABLE_FAILED",
                error.to_string(),
            )
        })?;
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_context, &trigger_id)
        .await;
    append_webhook_audit(
        &state,
        "automation.webhook_trigger.disabled",
        &tenant_context,
        audit_actor(&tenant_context, &request_principal, verified),
        json!({
            "automationID": id,
            "triggerID": trigger_id,
        }),
    )
    .await;
    Ok(Json(json!({
        "ok": true,
        "trigger": trigger_value(&updated, &deliveries, &headers),
    })))
}

async fn delete_webhook_trigger(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((id, trigger_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let _automation = load_automation_for_mutation(
        &state,
        &tenant_context,
        &request_principal,
        verified,
        &headers,
        &id,
        true,
    )
    .await?;
    let _trigger =
        load_trigger_for_mutation(&state, &tenant_context, verified, &id, &trigger_id).await?;
    let deleted = state
        .delete_automation_webhook_trigger(&tenant_context, &trigger_id)
        .await
        .map_err(|error| {
            error_response(
                StatusCode::BAD_REQUEST,
                "AUTOMATION_WEBHOOK_DELETE_FAILED",
                error.to_string(),
            )
        })?;
    append_webhook_audit(
        &state,
        "automation.webhook_trigger.deleted",
        &tenant_context,
        audit_actor(&tenant_context, &request_principal, verified),
        json!({
            "automationID": id,
            "triggerID": trigger_id,
            "deleted": deleted,
        }),
    )
    .await;
    Ok(Json(json!({
        "ok": true,
        "deleted": deleted,
        "trigger_id": trigger_id,
        "triggerID": trigger_id,
    })))
}

async fn rotate_webhook_secret(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((id, trigger_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let _automation = load_automation_for_mutation(
        &state,
        &tenant_context,
        &request_principal,
        verified,
        &headers,
        &id,
        false,
    )
    .await?;
    let _trigger =
        load_trigger_for_mutation(&state, &tenant_context, verified, &id, &trigger_id).await?;
    let actor_id = actor_id_for_records(&tenant_context, &request_principal, verified);
    let rotated = state
        .rotate_automation_webhook_secret(&tenant_context, &trigger_id, actor_id)
        .await
        .map_err(|error| {
            error_response(
                StatusCode::BAD_REQUEST,
                "AUTOMATION_WEBHOOK_ROTATE_FAILED",
                error.to_string(),
            )
        })?;
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_context, &trigger_id)
        .await;
    append_webhook_audit(
        &state,
        "automation.webhook_trigger.secret_rotated",
        &tenant_context,
        audit_actor(&tenant_context, &request_principal, verified),
        json!({
            "automationID": id,
            "triggerID": trigger_id,
            "secretVersion": rotated.trigger.secret.secret_version,
        }),
    )
    .await;
    Ok(Json(json!({
        "trigger": trigger_value(&rotated.trigger, &deliveries, &headers),
        "new_secret": rotated.secret,
        "newSecret": rotated.secret,
        "secret_one_time": true,
        "secretOneTime": true,
    })))
}

async fn list_webhook_deliveries(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path((id, trigger_id)): Path<(String, String)>,
    Query(query): Query<DeliveryListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let _automation = load_automation_for_read(&state, &tenant_context, verified, &id).await?;
    let _trigger =
        load_trigger_for_read(&state, &tenant_context, verified, &id, &trigger_id).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, 100);
    let mut deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_context, &trigger_id)
        .await;
    deliveries.sort_by(|left, right| right.received_at_ms.cmp(&left.received_at_ms));
    let rows = deliveries
        .iter()
        .take(limit)
        .map(delivery_value)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "deliveries": rows,
        "count": rows.len(),
        "limit": limit,
    })))
}

async fn get_webhook_delivery(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path((id, trigger_id, delivery_id)): Path<(String, String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let _automation = load_automation_for_read(&state, &tenant_context, verified, &id).await?;
    let _trigger =
        load_trigger_for_read(&state, &tenant_context, verified, &id, &trigger_id).await?;
    let Some(delivery) = state
        .get_automation_webhook_delivery(&tenant_context, &delivery_id)
        .await
    else {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "AUTOMATION_WEBHOOK_DELIVERY_NOT_FOUND",
            "Webhook delivery not found",
        ));
    };
    if delivery.trigger_id != trigger_id || delivery.automation_id != id {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "AUTOMATION_WEBHOOK_DELIVERY_NOT_FOUND",
            "Webhook delivery not found",
        ));
    }
    Ok(Json(json!({
        "delivery": delivery_value(&delivery),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_types::{
        AssertionMetadata, AuthorityChain, DataBoundary, GrantSource, HumanActor, ResourceKind,
        ResourceRef, ScopedGrant, StrictTenantContext,
    };

    fn verified_with_strict_grant(
        permissions: Vec<AccessPermission>,
        data_classes: Vec<DataClass>,
    ) -> (VerifiedTenantContext, ResourceScope) {
        let tenant_context =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "actor-a");
        let principal = PrincipalRef::human_user("actor-a");
        let request_principal = RequestPrincipal::authenticated_user("actor-a", "tandem-web");
        let authority_chain = AuthorityChain::from_request(request_principal);
        let resource = ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::Project,
            "automation-project",
        );
        let scope = ResourceScope::root(resource.clone());
        let grant = ScopedGrant::new(
            "grant-webhook-scope",
            principal.clone(),
            resource,
            GrantSource::Delegation,
        )
        .with_permissions(permissions)
        .with_data_classes(data_classes.clone());
        let strict_projection = StrictTenantContext::new(
            tenant_context.clone(),
            principal,
            authority_chain.clone(),
            scope.clone(),
            AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                9_999_999_999_999,
                "assertion-webhook-scope",
            ),
        )
        .with_grants(vec![grant])
        .with_data_boundary(DataBoundary::allow(data_classes));
        let verified = VerifiedTenantContext {
            tenant_context,
            human_actor: HumanActor::tandem_user("actor-a"),
            authority_chain,
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: Some(strict_projection),
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 9_999_999_999_999,
            assertion_id: "assertion-webhook-scope".to_string(),
            assertion_key_id: None,
        };
        (verified, scope)
    }

    #[test]
    fn strict_scope_allows_requires_matching_permission_grant() {
        let (viewer, scope) =
            verified_with_strict_grant(vec![AccessPermission::View], vec![DataClass::Internal]);
        assert!(strict_scope_allows(
            &viewer,
            &scope,
            AccessPermission::View,
            DataClass::Internal,
        ));
        assert!(!strict_scope_allows(
            &viewer,
            &scope,
            AccessPermission::Edit,
            DataClass::Internal,
        ));
        assert!(!strict_scope_allows(
            &viewer,
            &scope,
            AccessPermission::Admin,
            DataClass::Internal,
        ));

        let (admin, scope) =
            verified_with_strict_grant(vec![AccessPermission::Admin], vec![DataClass::Internal]);
        assert!(strict_scope_allows(
            &admin,
            &scope,
            AccessPermission::Edit,
            DataClass::Internal,
        ));
        assert!(strict_scope_allows(
            &admin,
            &scope,
            AccessPermission::Admin,
            DataClass::Internal,
        ));
    }

    #[test]
    fn strict_scope_allows_requires_matching_data_class() {
        let (verified, scope) =
            verified_with_strict_grant(vec![AccessPermission::Admin], vec![DataClass::Internal]);
        assert!(!strict_scope_allows(
            &verified,
            &scope,
            AccessPermission::Admin,
            DataClass::Confidential,
        ));
    }
}
