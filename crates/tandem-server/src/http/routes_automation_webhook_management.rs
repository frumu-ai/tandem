// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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
    AutomationWebhookRawEventRecord, AutomationWebhookSignatureScheme,
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
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/reveal-verification-token",
            post(reveal_webhook_verification_token),
        )
        .route(
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/reset-verification",
            post(reset_webhook_verification),
        )
        .route(
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/import-secret",
            post(import_webhook_provider_secret),
        )
        .route(
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/deliveries",
            get(list_webhook_deliveries),
        )
        .route(
            "/automations/v2/{id}/webhook-triggers/{trigger_id}/deliveries/{delivery_id}",
            get(get_webhook_delivery),
        )
        .route("/automations/v2/webhook-events", get(list_webhook_events))
        .route(
            "/automations/v2/webhook-events/{event_id}",
            get(get_webhook_event),
        )
        .route(
            "/automations/v2/runs/{run_id}/webhook-events",
            get(list_webhook_events_for_run),
        )
}

#[derive(Default, Deserialize)]
struct WebhookTriggerCreateRequest {
    #[serde(default)]
    name: Option<String>,
    provider: String,
    #[serde(default)]
    provider_event_kind: Option<String>,
    #[serde(default, alias = "signatureScheme")]
    signature_scheme: Option<AutomationWebhookSignatureScheme>,
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
    #[serde(default, alias = "signatureScheme")]
    signature_scheme: Option<AutomationWebhookSignatureScheme>,
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

#[derive(Default, Deserialize)]
struct WebhookEventListQuery {
    #[serde(default, alias = "triggerID")]
    trigger_id: Option<String>,
    #[serde(default, alias = "automationID")]
    automation_id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Default, Deserialize)]
struct WebhookEventDetailQuery {
    #[serde(default, alias = "includePayload")]
    include_payload: Option<bool>,
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

fn webhook_event_not_found() -> (StatusCode, Json<Value>) {
    error_response(
        StatusCode::NOT_FOUND,
        "AUTOMATION_WEBHOOK_EVENT_NOT_FOUND",
        "Webhook event not found",
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
        state
            .can_mutate_automation(id, &actor, delete_intent, tenant_context)
            .await,
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

async fn require_webhook_event_read(
    state: &AppState,
    tenant_context: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    event: &AutomationWebhookRawEventRecord,
) -> Result<(), (StatusCode, Json<Value>)> {
    load_automation_for_read(state, tenant_context, verified, &event.automation_id)
        .await
        .map_err(|_| webhook_event_not_found())?;
    load_trigger_for_read(
        state,
        tenant_context,
        verified,
        &event.automation_id,
        &event.trigger_id,
    )
    .await
    .map_err(|_| webhook_event_not_found())?;
    Ok(())
}

fn trigger_display_name(trigger: &AutomationWebhookTriggerRecord) -> String {
    let name = trigger.name.trim();
    if name.is_empty() {
        trigger.provider.clone()
    } else {
        name.to_string()
    }
}

fn header_string(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn callback_path(headers: &HeaderMap, trigger: &AutomationWebhookTriggerRecord) -> String {
    let forwarded_prefix = header_string(headers, "x-forwarded-prefix")
        .map(|value| value.trim_end_matches('/').to_string())
        .filter(|value| value == "/api/engine")
        .unwrap_or_default();
    format!(
        "{forwarded_prefix}/webhooks/automations/{}",
        trigger.public_path_token
    )
}

fn callback_url_from_path(headers: &HeaderMap, path: String) -> String {
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

fn callback_url(headers: &HeaderMap, trigger: &AutomationWebhookTriggerRecord) -> String {
    callback_url_from_path(headers, callback_path(headers, trigger))
}

fn notion_setup_callback_path(
    headers: &HeaderMap,
    trigger: &AutomationWebhookTriggerRecord,
    setup_nonce: &str,
) -> String {
    format!("{}/{}", callback_path(headers, trigger), setup_nonce)
}

fn notion_setup_callback_url(
    headers: &HeaderMap,
    trigger: &AutomationWebhookTriggerRecord,
    setup_nonce: &str,
) -> String {
    callback_url_from_path(
        headers,
        notion_setup_callback_path(headers, trigger, setup_nonce),
    )
}

fn delivery_status_key(status: &AutomationWebhookDeliveryStatus) -> &'static str {
    match status {
        AutomationWebhookDeliveryStatus::Received => "received",
        AutomationWebhookDeliveryStatus::Accepted => "accepted",
        AutomationWebhookDeliveryStatus::Rejected => "rejected",
        AutomationWebhookDeliveryStatus::Duplicate => "duplicate",
        AutomationWebhookDeliveryStatus::Suppressed => "suppressed",
        AutomationWebhookDeliveryStatus::Disabled => "disabled",
        AutomationWebhookDeliveryStatus::Failed => "failed",
    }
}

fn webhook_status_from_key(status: &str) -> Option<AutomationWebhookDeliveryStatus> {
    match status.trim().to_ascii_lowercase().as_str() {
        "received" => Some(AutomationWebhookDeliveryStatus::Received),
        "accepted" => Some(AutomationWebhookDeliveryStatus::Accepted),
        "rejected" => Some(AutomationWebhookDeliveryStatus::Rejected),
        "duplicate" => Some(AutomationWebhookDeliveryStatus::Duplicate),
        "suppressed" => Some(AutomationWebhookDeliveryStatus::Suppressed),
        "disabled" => Some(AutomationWebhookDeliveryStatus::Disabled),
        "failed" | "dead_letter" | "dead-letter" => Some(AutomationWebhookDeliveryStatus::Failed),
        _ => None,
    }
}

fn delivery_counts(deliveries: &[AutomationWebhookDeliveryRecord]) -> Value {
    let mut received = 0usize;
    let mut accepted = 0usize;
    let mut rejected = 0usize;
    let mut duplicate = 0usize;
    let mut suppressed = 0usize;
    let mut disabled = 0usize;
    let mut failed = 0usize;
    for delivery in deliveries {
        match delivery.status {
            AutomationWebhookDeliveryStatus::Received => received += 1,
            AutomationWebhookDeliveryStatus::Accepted => accepted += 1,
            AutomationWebhookDeliveryStatus::Rejected => rejected += 1,
            AutomationWebhookDeliveryStatus::Duplicate => duplicate += 1,
            AutomationWebhookDeliveryStatus::Suppressed => suppressed += 1,
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
        "suppressed": suppressed,
        "disabled": disabled,
        "failed": failed,
    })
}

fn provider_metadata(trigger: &AutomationWebhookTriggerRecord) -> Value {
    let canonical_provider = normalize_automation_webhook_provider(&trigger.provider)
        .unwrap_or_else(|| "generic".to_string());
    let event_id_headers = automation_webhook_provider_event_id_headers(&canonical_provider);
    let provider_specific_verification = matches!(
        trigger.signature_scheme,
        AutomationWebhookSignatureScheme::GithubHmacSha256
            | AutomationWebhookSignatureScheme::NotionHmacSha256
            | AutomationWebhookSignatureScheme::LinearHmacSha256
    );
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
            "provider_specific": provider_specific_verification,
            "providerSpecific": provider_specific_verification,
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
        "callback_path": callback_path(headers, trigger),
        "callbackPath": callback_path(headers, trigger),
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
            "configured": trigger_secret_configured(trigger),
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
        "verification_status": provider_verification_value(trigger),
        "verificationStatus": provider_verification_value(trigger),
        "delivery_counts": delivery_counts(deliveries),
        "deliveryCounts": delivery_counts(deliveries),
    })
}

/// Whether the trigger has usable signing-secret material. Always true for
/// Tandem-generated secrets; false for a Linear trigger whose provider-owned
/// signing secret has not been imported yet (deliveries fail closed until then).
fn trigger_secret_configured(trigger: &AutomationWebhookTriggerRecord) -> bool {
    match trigger.linear_verification.as_ref() {
        Some(verification) => verification.secret_configured(),
        None => true,
    }
}

/// Provider-owned verification state for the Control Panel — never includes
/// secret material, only lifecycle status.
fn provider_verification_value(trigger: &AutomationWebhookTriggerRecord) -> Value {
    if let Some(verification) = trigger.notion_verification.as_ref() {
        return json!({
            "provider": "notion",
            "status": verification.status.as_str(),
            "token_available": verification.token_available_for_reveal(),
            "tokenAvailable": verification.token_available_for_reveal(),
            "token_received_at_ms": verification.token_received_at_ms,
            "tokenReceivedAtMs": verification.token_received_at_ms,
            "token_revealed_at_ms": verification.token_revealed_at_ms,
            "tokenRevealedAtMs": verification.token_revealed_at_ms,
            "verified_at_ms": verification.verified_at_ms,
            "verifiedAtMs": verification.verified_at_ms,
        });
    }
    if let Some(verification) = trigger.linear_verification.as_ref() {
        return json!({
            "provider": "linear",
            "status": verification.status.as_str(),
            "secret_configured": verification.secret_configured(),
            "secretConfigured": verification.secret_configured(),
            "secret_imported_at_ms": verification.secret_imported_at_ms,
            "secretImportedAtMs": verification.secret_imported_at_ms,
            "verified_at_ms": verification.verified_at_ms,
            "verifiedAtMs": verification.verified_at_ms,
        });
    }
    Value::Null
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
        "status": delivery_status_key(&delivery.status),
        "rejection_reason_code": delivery.rejection_reason_code,
        "rejectionReasonCode": delivery.rejection_reason_code,
        "idempotency_key": delivery.idempotency_key,
        "idempotencyKey": delivery.idempotency_key,
        "idempotency_record_id": delivery.idempotency_record_id,
        "idempotencyRecordID": delivery.idempotency_record_id,
        "dedupe_result": delivery.dedupe_result,
        "dedupeResult": delivery.dedupe_result,
        "dedupe_reason_code": delivery.dedupe_reason_code,
        "dedupeReasonCode": delivery.dedupe_reason_code,
        "duplicate_of_delivery_id": delivery.duplicate_of_delivery_id,
        "duplicateOfDeliveryID": delivery.duplicate_of_delivery_id,
        "duplicate_of_run_id": delivery.duplicate_of_run_id,
        "duplicateOfRunID": delivery.duplicate_of_run_id,
        "verification_scheme": delivery.verification_scheme,
        "verificationScheme": delivery.verification_scheme,
        "verification_provider": delivery.verification_provider,
        "verificationProvider": delivery.verification_provider,
        "verification_reason_code": delivery.verification_reason_code,
        "verificationReasonCode": delivery.verification_reason_code,
        "queued_run_id": delivery.queued_run_id,
        "queuedRunID": delivery.queued_run_id,
        "queued_run_path": delivery.queued_run_id.as_ref().map(|run_id| format!("/automations/v2/runs/{run_id}")),
        "queuedRunPath": delivery.queued_run_id.as_ref().map(|run_id| format!("/automations/v2/runs/{run_id}")),
        "woken_run_id": delivery.woken_run_id,
        "wokenRunID": delivery.woken_run_id,
        "woken_wait_id": delivery.woken_wait_id,
        "wokenWaitID": delivery.woken_wait_id,
        "feedback_loop": delivery.feedback_loop,
        "feedbackLoop": delivery.feedback_loop,
        "correlation": delivery.correlation,
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

fn raw_event_value(event: &AutomationWebhookRawEventRecord, payload: Option<Value>) -> Value {
    let payload_available = event
        .retention_policy
        .delete_after_ms
        .is_none_or(|delete_after_ms| crate::now_ms() <= delete_after_ms);
    json!({
        "event_id": event.event_id,
        "eventID": event.event_id,
        "trigger_id": event.trigger_id,
        "triggerID": event.trigger_id,
        "automation_id": event.automation_id,
        "automationID": event.automation_id,
        "provider": event.provider,
        "provider_event_kind": event.provider_event_kind,
        "providerEventKind": event.provider_event_kind,
        "provider_event_id": event.provider_event_id,
        "providerEventID": event.provider_event_id,
        "body_digest": event.body_digest,
        "bodyDigest": event.body_digest,
        "headers_digest": event.headers_digest,
        "headersDigest": event.headers_digest,
        "headers_redacted": event.headers_redacted,
        "headersRedacted": event.headers_redacted,
        "content_type": event.content_type,
        "contentType": event.content_type,
        "payload_ref": event.payload_ref,
        "payloadRef": event.payload_ref,
        "payload_bytes": event.payload_bytes,
        "payloadBytes": event.payload_bytes,
        "payload_available": payload_available,
        "payloadAvailable": payload_available,
        "payload": payload,
        "status": delivery_status_key(&event.status),
        "received_at_ms": event.received_at_ms,
        "receivedAtMs": event.received_at_ms,
        "updated_at_ms": event.updated_at_ms,
        "updatedAtMs": event.updated_at_ms,
        "delivery_id": event.delivery_id,
        "deliveryID": event.delivery_id,
        "queued_run_id": event.queued_run_id,
        "queuedRunID": event.queued_run_id,
        "rejection_reason_code": event.rejection_reason_code,
        "rejectionReasonCode": event.rejection_reason_code,
        "idempotency_key": event.idempotency_key,
        "idempotencyKey": event.idempotency_key,
        "idempotency_record_id": event.idempotency_record_id,
        "idempotencyRecordID": event.idempotency_record_id,
        "dedupe_result": event.dedupe_result,
        "dedupeResult": event.dedupe_result,
        "dedupe_reason_code": event.dedupe_reason_code,
        "dedupeReasonCode": event.dedupe_reason_code,
        "duplicate_of_delivery_id": event.duplicate_of_delivery_id,
        "duplicateOfDeliveryID": event.duplicate_of_delivery_id,
        "duplicate_of_run_id": event.duplicate_of_run_id,
        "duplicateOfRunID": event.duplicate_of_run_id,
        "woken_run_id": event.woken_run_id,
        "wokenRunID": event.woken_run_id,
        "woken_wait_id": event.woken_wait_id,
        "wokenWaitID": event.woken_wait_id,
        "feedback_loop": event.feedback_loop,
        "feedbackLoop": event.feedback_loop,
        "correlation": event.correlation,
        "retention_policy": event.retention_policy,
        "retentionPolicy": event.retention_policy,
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
) -> Result<(), (StatusCode, Json<Value>)> {
    crate::audit::append_protected_audit_event(state, event_type, tenant_context, actor, details)
        .await
        .map_err(super::protected_audit_error_response)
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
            signature_scheme: input.signature_scheme,
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
            "signatureScheme": result.trigger.signature_scheme,
        }),
    )
    .await?;
    // Notion and Linear are provider-owned-secret flows: the real signing secret
    // is Notion's later-POSTed verification token or Linear's UI-displayed
    // signing secret (imported by the operator), so we never reveal the
    // placeholder secret generated at creation — surfacing it would invite the
    // operator to paste a value the provider can never sign with.
    if let Some(verification) = result.trigger.notion_verification.as_ref() {
        let Some(setup_nonce) = result.notion_setup_nonce.as_deref() else {
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "AUTOMATION_WEBHOOK_NOTION_SETUP_INVARIANT",
                "Notion trigger creation did not produce a setup challenge",
            ));
        };
        return Ok(Json(json!({
            "trigger": trigger_value(&result.trigger, &[], &headers),
            "secret_one_time": false,
            "secretOneTime": false,
            "verification_pending": true,
            "verificationPending": true,
            "setup_callback_path": notion_setup_callback_path(&headers, &result.trigger, setup_nonce),
            "setupCallbackPath": notion_setup_callback_path(&headers, &result.trigger, setup_nonce),
            "setup_callback_url": notion_setup_callback_url(&headers, &result.trigger, setup_nonce),
            "setupCallbackUrl": notion_setup_callback_url(&headers, &result.trigger, setup_nonce),
            "setup_expires_at_ms": verification.setup_challenge_expires_at_ms,
            "setupExpiresAtMs": verification.setup_challenge_expires_at_ms,
        })));
    }
    if result.trigger.linear_verification.is_some() {
        return Ok(Json(json!({
            "trigger": trigger_value(&result.trigger, &[], &headers),
            "secret_one_time": false,
            "secretOneTime": false,
            "verification_pending": true,
            "verificationPending": true,
        })));
    }
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
                signature_scheme: input.signature_scheme,
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
    .await?;
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
    .await?;
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
    .await?;
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
    let trigger =
        load_trigger_for_mutation(&state, &tenant_context, verified, &id, &trigger_id).await?;
    // Notion and Linear triggers sign with a provider-owned secret (Notion's
    // verification token, Linear's signing secret); rotating to a
    // Tandem-generated secret would break verification. Linear secrets are
    // replaced by re-importing from Linear's UI instead.
    if trigger.is_provider_owned_secret() {
        return Err(error_response(
            StatusCode::BAD_REQUEST,
            "AUTOMATION_WEBHOOK_ROTATE_UNSUPPORTED",
            "provider-owned-secret (notion/linear) webhook triggers cannot rotate a Tandem secret",
        ));
    }
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
    .await?;
    Ok(Json(json!({
        "trigger": trigger_value(&rotated.trigger, &deliveries, &headers),
        "new_secret": rotated.secret,
        "newSecret": rotated.secret,
        "secret_one_time": true,
        "secretOneTime": true,
    })))
}

/// One-time reveal of a Notion trigger's provider-supplied verification token so
/// an authorized operator can paste it back into Notion. Admin-scoped; the token
/// is returned at most once and never again.
async fn reveal_webhook_verification_token(
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
    let token = state
        .reveal_automation_webhook_notion_verification_token(&tenant_context, &id, &trigger_id)
        .await
        .map_err(|error| {
            error_response(
                StatusCode::BAD_REQUEST,
                "AUTOMATION_WEBHOOK_REVEAL_FAILED",
                error.to_string(),
            )
        })?;
    let Some(token) = token else {
        return Err(error_response(
            StatusCode::CONFLICT,
            "AUTOMATION_WEBHOOK_TOKEN_UNAVAILABLE",
            "no verification token is available to reveal for this trigger",
        ));
    };
    append_webhook_audit(
        &state,
        "automation.webhook_trigger.verification_token_revealed",
        &tenant_context,
        audit_actor(&tenant_context, &request_principal, verified),
        json!({ "automationID": id, "triggerID": trigger_id }),
    )
    .await?;
    let trigger = state
        .get_automation_webhook_trigger(&tenant_context, &trigger_id)
        .await;
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_context, &trigger_id)
        .await;
    Ok(Json(json!({
        "verification_token": token,
        "verificationToken": token,
        "token_one_time": true,
        "tokenOneTime": true,
        "trigger": trigger.map(|trigger| trigger_value(&trigger, &deliveries, &headers)),
    })))
}

/// Rotate any prior Notion token out of service and issue a fresh setup URL.
/// The mutation is authenticated by the same automation/trigger authorization
/// checks as create, rotate, and reveal, and the nonce itself is returned once.
async fn reset_webhook_verification(
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
    let actor = actor_id_for_records(&tenant_context, &request_principal, verified);
    let reset = state
        .reset_automation_webhook_notion_setup(&tenant_context, &id, &trigger_id, actor.clone())
        .await
        .map_err(|error| {
            error_response(
                StatusCode::BAD_REQUEST,
                "AUTOMATION_WEBHOOK_RESET_VERIFICATION_FAILED",
                error.to_string(),
            )
        })?;
    append_webhook_audit(
        &state,
        "automation.webhook_trigger.verification_reset",
        &tenant_context,
        audit_actor(&tenant_context, &request_principal, verified),
        json!({
            "automationID": id,
            "triggerID": trigger_id,
            "secretVersion": reset.trigger.secret.secret_version,
            "setupGeneration": reset
                .trigger
                .notion_verification
                .as_ref()
                .map(|verification| verification.setup_generation),
        }),
    )
    .await?;
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_context, &trigger_id)
        .await;
    let expires_at_ms = reset
        .trigger
        .notion_verification
        .as_ref()
        .and_then(|verification| verification.setup_challenge_expires_at_ms);
    Ok(Json(json!({
        "trigger": trigger_value(&reset.trigger, &deliveries, &headers),
        "setup_callback_path": notion_setup_callback_path(&headers, &reset.trigger, &reset.setup_nonce),
        "setupCallbackPath": notion_setup_callback_path(&headers, &reset.trigger, &reset.setup_nonce),
        "setup_callback_url": notion_setup_callback_url(&headers, &reset.trigger, &reset.setup_nonce),
        "setupCallbackUrl": notion_setup_callback_url(&headers, &reset.trigger, &reset.setup_nonce),
        "setup_expires_at_ms": expires_at_ms,
        "setupExpiresAtMs": expires_at_ms,
        "setup_one_time": true,
        "setupOneTime": true,
    })))
}

#[derive(Deserialize)]
struct WebhookProviderSecretImportRequest {
    secret: String,
}

/// Import (or replace) a provider-owned signing secret for a trigger — the
/// operator pastes the secret shown in the provider's UI (Linear webhook
/// settings) into Tandem. The inverse of the Notion reveal flow: the secret
/// flows into Tandem and is never returned by any endpoint afterwards.
async fn import_webhook_provider_secret(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((id, trigger_id)): Path<(String, String)>,
    Json(request): Json<WebhookProviderSecretImportRequest>,
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
    let trigger = state
        .import_automation_webhook_linear_secret(
            &tenant_context,
            &id,
            &trigger_id,
            &request.secret,
            actor_id,
        )
        .await
        .map_err(|error| {
            error_response(
                StatusCode::BAD_REQUEST,
                "AUTOMATION_WEBHOOK_SECRET_IMPORT_FAILED",
                error.to_string(),
            )
        })?;
    let deliveries = state
        .list_automation_webhook_deliveries_for_trigger(&tenant_context, &trigger_id)
        .await;
    append_webhook_audit(
        &state,
        "automation.webhook_trigger.secret_imported",
        &tenant_context,
        audit_actor(&tenant_context, &request_principal, verified),
        json!({
            "automationID": id,
            "triggerID": trigger_id,
            "provider": trigger.provider,
            "secretVersion": trigger.secret.secret_version,
        }),
    )
    .await?;
    Ok(Json(json!({
        "ok": true,
        "trigger": trigger_value(&trigger, &deliveries, &headers),
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

async fn list_webhook_events(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<WebhookEventListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let status = match query.status.as_deref() {
        Some(status) => Some(webhook_status_from_key(status).ok_or_else(|| {
            error_response(
                StatusCode::BAD_REQUEST,
                "AUTOMATION_WEBHOOK_EVENT_STATUS_INVALID",
                "Unknown webhook event status",
            )
        })?),
        None => None,
    };
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let events = state
        .list_automation_webhook_raw_events(
            &tenant_context,
            query.trigger_id.as_deref(),
            query.automation_id.as_deref(),
            status,
            200,
        )
        .await;
    let mut rows = Vec::new();
    for event in events {
        if rows.len() >= limit {
            break;
        }
        if require_webhook_event_read(&state, &tenant_context, verified, &event)
            .await
            .is_ok()
        {
            rows.push(raw_event_value(&event, None));
        }
    }
    Ok(Json(json!({
        "events": rows,
        "count": rows.len(),
        "limit": limit,
    })))
}

async fn get_webhook_event(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(event_id): Path<String>,
    Query(query): Query<WebhookEventDetailQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let event = state
        .get_automation_webhook_raw_event(&tenant_context, &event_id)
        .await
        .map_err(|error| {
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "AUTOMATION_WEBHOOK_EVENT_READ_FAILED",
                error.to_string(),
            )
        })?
        .ok_or_else(|| {
            error_response(
                StatusCode::NOT_FOUND,
                "AUTOMATION_WEBHOOK_EVENT_NOT_FOUND",
                "Webhook event not found",
            )
        })?;
    require_webhook_event_read(&state, &tenant_context, verified, &event).await?;
    let payload_available = event
        .retention_policy
        .delete_after_ms
        .is_none_or(|delete_after_ms| crate::now_ms() <= delete_after_ms);
    let payload = if query.include_payload.unwrap_or(false) && payload_available {
        state
            .read_automation_webhook_raw_event_payload(&tenant_context, &event.event_id)
            .await
            .map_err(|error| {
                error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "AUTOMATION_WEBHOOK_EVENT_PAYLOAD_READ_FAILED",
                    error.to_string(),
                )
            })?
            .map(|payload| {
                serde_json::from_slice::<Value>(&payload).unwrap_or_else(|_| {
                    Value::String(String::from_utf8_lossy(&payload).to_string())
                })
            })
    } else {
        None
    };
    Ok(Json(json!({
        "event": raw_event_value(&event, payload),
    })))
}

async fn list_webhook_events_for_run(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(run_id): Path<String>,
    Query(query): Query<DeliveryListQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let run = state.get_automation_v2_run(&run_id).await.ok_or_else(|| {
        error_response(
            StatusCode::NOT_FOUND,
            "AUTOMATION_V2_RUN_NOT_FOUND",
            "Automation run not found",
        )
    })?;
    if run.tenant_context.org_id != tenant_context.org_id
        || run.tenant_context.workspace_id != tenant_context.workspace_id
        || run.tenant_context.deployment_id != tenant_context.deployment_id
    {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "AUTOMATION_V2_RUN_NOT_FOUND",
            "Automation run not found",
        ));
    }
    load_automation_for_read(&state, &tenant_context, verified, &run.automation_id)
        .await
        .map_err(|_| {
            error_response(
                StatusCode::NOT_FOUND,
                "AUTOMATION_V2_RUN_NOT_FOUND",
                "Automation run not found",
            )
        })?;
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let events = state
        .list_automation_webhook_raw_events_for_run(&tenant_context, &run_id, limit)
        .await;
    let mut rows = Vec::new();
    for event in events {
        if require_webhook_event_read(&state, &tenant_context, verified, &event)
            .await
            .is_ok()
        {
            rows.push(raw_event_value(&event, None));
        }
    }
    Ok(Json(json!({
        "run_id": run_id,
        "runID": run_id,
        "events": rows,
        "count": rows.len(),
        "limit": limit,
    })))
}

#[cfg(test)]
#[path = "routes_automation_webhook_management_tests.rs"]
mod tests;
