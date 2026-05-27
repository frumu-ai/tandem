use std::collections::HashMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    ConnectorCredentialClass, ConnectorCredentialRef, ConnectorInstance, ConnectorLifecycleState,
    DataClass, IngestionJob, IngestionPolicy, IngestionQuarantine, PrincipalRef, RequestPrincipal,
    ResourceRef, SecretRef, SourceBinding, SourceBindingState, TenantContext,
    VerifiedTenantContext,
};
use tandem_memory::db::MemoryDatabase;
use tandem_memory::response_cache::ResponseCache;
use tandem_memory::types::{
    MemoryTenantScope, SourceObjectLifecycleRecord, SourceObjectLifecycleState,
};
use tandem_server::{now_ms, AppState, EngineEvent};

pub(super) type EnterpriseResult<T> = Result<Json<T>, (StatusCode, Json<Value>)>;

const GOOGLE_DRIVE_PROVIDER: &str = "google_drive";
const GOOGLE_DRIVE_SOURCE_TYPE: &str = "google_drive";

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseAdminResponseBase {
    pub(super) tenant_context: TenantContext,
    pub(super) request_principal: RequestPrincipal,
    pub(super) bridge_state: &'static str,
    pub(super) status: &'static str,
    pub(super) message: &'static str,
}

#[derive(Debug, Serialize)]
struct EnterpriseConnectorProvidersResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    providers: Vec<EnterpriseConnectorProviderDescriptor>,
    count: usize,
}

#[derive(Debug, Serialize)]
struct EnterpriseConnectorProviderDescriptor {
    provider: &'static str,
    display_name: &'static str,
    status: &'static str,
    default_credential_class: ConnectorCredentialClass,
    supported_credential_classes: Vec<ConnectorCredentialClass>,
    source_types: Vec<&'static str>,
    source_root_mode: &'static str,
    acl_sync: &'static str,
    live_ingestion: &'static str,
}

#[derive(Debug, Serialize)]
struct EnterpriseSourceBindingsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    source_bindings: Vec<SourceBinding>,
    count: usize,
}

#[derive(Debug, Serialize)]
struct EnterpriseConnectorsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    connectors: Vec<ConnectorInstance>,
    count: usize,
}

#[derive(Debug, Serialize)]
struct EnterpriseConnectorImpactResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    connector_id: String,
    affected_bindings: Vec<SourceBinding>,
    affected_source_objects: Vec<SourceObjectLifecycleRecord>,
    affected_ingestion_jobs: Vec<IngestionJob>,
    affected_quarantines: Vec<IngestionQuarantine>,
    cache_invalidation_required: bool,
    compromise_window_started_at_ms: Option<u64>,
    compromise_window_finished_at_ms: Option<u64>,
    recommended_actions: Vec<&'static str>,
}

#[derive(Debug, Deserialize)]
struct CreateSourceBindingRequest {
    binding_id: String,
    connector_id: String,
    source_type: String,
    native_source_id: String,
    #[serde(default)]
    source_root_label: Option<String>,
    resource_ref: ResourceRef,
    data_class: DataClass,
    #[serde(default)]
    state: SourceBindingState,
    #[serde(default)]
    credential_ref_id: Option<String>,
    #[serde(default)]
    ingestion_policy: IngestionPolicy,
}

#[derive(Debug, Deserialize)]
struct CreateConnectorRequest {
    connector_id: String,
    provider: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    state: ConnectorLifecycleState,
}

#[derive(Debug, Deserialize)]
struct UpdateConnectorRequest {
    #[serde(default)]
    state: Option<ConnectorLifecycleState>,
    #[serde(default)]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateConnectorCredentialRefRequest {
    credential_id: String,
    #[serde(default)]
    credential_class: ConnectorCredentialClass,
    secret_ref: SecretRef,
    #[serde(default)]
    source_bound_resource: Option<ResourceRef>,
    #[serde(default)]
    expires_at_ms: Option<u64>,
    #[serde(default)]
    credential_value: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RotateConnectorCredentialRefRequest {
    secret_ref: SecretRef,
    #[serde(default)]
    expires_at_ms: Option<u64>,
    #[serde(default)]
    credential_value: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct UpdateSourceBindingRequest {
    #[serde(default)]
    state: Option<SourceBindingState>,
    #[serde(default)]
    source_root_label: Option<String>,
    #[serde(default)]
    credential_ref_id: Option<String>,
    #[serde(default)]
    ingestion_policy: Option<IngestionPolicy>,
}

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    super::routes_enterprise_onboarding::apply(super::routes_enterprise_org_units::apply(router))
        .route(
            "/enterprise/connector-providers",
            get(list_connector_providers),
        )
        .route(
            "/enterprise/source-bindings",
            get(list_source_bindings).post(create_source_binding),
        )
        .route(
            "/enterprise/connectors",
            get(list_connectors).post(create_connector),
        )
        .route(
            "/enterprise/connectors/{connector_id}",
            patch(update_connector),
        )
        .route(
            "/enterprise/connectors/{connector_id}/impact",
            get(get_connector_impact),
        )
        .route(
            "/enterprise/connectors/{connector_id}/credential-refs",
            post(create_connector_credential_ref),
        )
        .route(
            "/enterprise/connectors/{connector_id}/credential-refs/{credential_id}/rotate",
            patch(rotate_connector_credential_ref),
        )
        .route(
            "/enterprise/ingestion-jobs",
            get(super::routes_enterprise_lifecycle::list_ingestion_jobs),
        )
        .route(
            "/enterprise/ingestion-quarantines",
            get(super::routes_enterprise_lifecycle::list_ingestion_quarantines),
        )
        .route(
            "/enterprise/ingestion-quarantines/{quarantine_id}/review",
            patch(super::routes_enterprise_lifecycle::review_ingestion_quarantine),
        )
        .route(
            "/enterprise/source-bindings/{binding_id}",
            patch(update_source_binding),
        )
        .route(
            "/enterprise/source-bindings/{binding_id}/google-drive/preflight",
            post(super::routes_enterprise_google_drive::preflight_google_drive_source_binding),
        )
        .route(
            "/enterprise/source-bindings/{binding_id}/google-drive/import",
            post(super::routes_enterprise_google_drive::import_google_drive_source_binding),
        )
        .route(
            "/enterprise/source-bindings/{binding_id}/google-drive/reindex",
            post(super::routes_enterprise_google_drive::reindex_google_drive_source_binding),
        )
        .route(
            "/enterprise/source-bindings/{binding_id}/source-objects",
            get(super::routes_enterprise_lifecycle::list_source_objects),
        )
        .route(
            "/enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}/reindex",
            post(super::routes_enterprise_lifecycle::reindex_source_object),
        )
        .route(
            "/enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}",
            axum::routing::delete(super::routes_enterprise_lifecycle::delete_source_object),
        )
        .route(
            "/enterprise/source-bindings/{binding_id}/source-objects/{source_object_id}/scope",
            patch(super::routes_enterprise_lifecycle::rescope_source_object),
        )
}

async fn list_connector_providers(
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseConnectorProvidersResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let providers = vec![EnterpriseConnectorProviderDescriptor {
        provider: GOOGLE_DRIVE_PROVIDER,
        display_name: "Google Drive",
        status: "planned_read_only",
        default_credential_class: ConnectorCredentialClass::ReadOnly,
        supported_credential_classes: vec![ConnectorCredentialClass::ReadOnly],
        source_types: vec![GOOGLE_DRIVE_SOURCE_TYPE],
        source_root_mode: "admin_labeled",
        acl_sync: "not_synced_v1",
        live_ingestion: "disabled_until_fetcher_worker",
    }];

    Ok(Json(EnterpriseConnectorProvidersResponse {
        count: providers.len(),
        providers,
        base: storage_base(tenant_context, request_principal),
    }))
}

async fn list_source_bindings(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> Json<EnterpriseSourceBindingsResponse> {
    let mut source_bindings: Vec<_> = state
        .enterprise_source_bindings
        .read()
        .await
        .values()
        .filter(|binding| binding.tenant_matches(&tenant_context))
        .cloned()
        .collect();
    source_bindings.sort_by(|left, right| left.binding_id.cmp(&right.binding_id));

    Json(EnterpriseSourceBindingsResponse {
        base: storage_base(tenant_context, request_principal),
        count: source_bindings.len(),
        source_bindings,
    })
}

async fn create_source_binding(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<CreateSourceBindingRequest>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;

    let binding_id = validate_enterprise_id("binding_id", &input.binding_id)?;
    let connector_id = validate_enterprise_id("connector_id", &input.connector_id)?;
    let source_type = validate_enterprise_id("source_type", &input.source_type)?;
    let native_source_id = validate_external_id("native_source_id", &input.native_source_id)?;
    validate_resource_ref_matches_tenant(&input.resource_ref, &tenant_context)?;
    validate_google_drive_source_binding_policy(
        &connector_id,
        &source_type,
        &input.ingestion_policy,
    )?;
    let credential_ref_id = input
        .credential_ref_id
        .as_deref()
        .map(|value| validate_enterprise_id("credential_ref_id", value))
        .transpose()?;
    let actor_id = request_principal
        .actor_id
        .clone()
        .unwrap_or_else(|| request_principal.source.clone());
    let mut binding = SourceBinding::enabled(
        binding_id,
        tenant_context.clone(),
        connector_id,
        source_type,
        native_source_id,
        input.resource_ref,
        input.data_class,
        PrincipalRef::human_user(actor_id),
        now_ms(),
    )
    .with_state(input.state, now_ms())
    .with_ingestion_policy(input.ingestion_policy);
    binding.source_root_label = normalized_optional_label(input.source_root_label);
    if let Some(credential_ref_id) = credential_ref_id {
        binding = binding.with_credential_ref_id(credential_ref_id);
    }

    {
        let mut registry = state.enterprise_source_bindings.write().await;
        registry.insert(enterprise_source_binding_key(&binding), binding);
        persist_enterprise_source_bindings(&state.enterprise_source_bindings_path, &registry)
            .await?;
    }
    emit_source_binding_cache_invalidation_required(
        &state,
        &tenant_context,
        &input.binding_id,
        "source_binding_created",
    );
    let _ =
        invalidate_response_cache_for_source_binding(&state, &tenant_context, &input.binding_id)
            .await?;

    Ok(Json(EnterpriseAdminResponseBase {
        message: "enterprise source binding saved",
        ..storage_base(tenant_context, request_principal)
    }))
}

async fn list_connectors(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> Json<EnterpriseConnectorsResponse> {
    let mut connectors: Vec<_> = state
        .enterprise_connectors
        .read()
        .await
        .values()
        .filter(|connector| connector.tenant_matches(&tenant_context))
        .cloned()
        .collect();
    connectors.sort_by(|left, right| left.connector_id.cmp(&right.connector_id));

    Json(EnterpriseConnectorsResponse {
        base: storage_base(tenant_context, request_principal),
        count: connectors.len(),
        connectors,
    })
}

async fn get_connector_impact(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseConnectorImpactResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let connector_id = validate_enterprise_id("connector_id", &connector_id)?;
    ensure_connector_exists_for_tenant(&state, &tenant_context, &connector_id).await?;
    let impact = build_connector_impact(&state, &tenant_context, &connector_id).await?;

    Ok(Json(EnterpriseConnectorImpactResponse {
        base: storage_base(tenant_context, request_principal),
        connector_id,
        affected_bindings: impact.affected_bindings,
        affected_source_objects: impact.affected_source_objects,
        affected_ingestion_jobs: impact.affected_ingestion_jobs,
        affected_quarantines: impact.affected_quarantines,
        cache_invalidation_required: impact.cache_invalidation_required,
        compromise_window_started_at_ms: impact.compromise_window_started_at_ms,
        compromise_window_finished_at_ms: impact.compromise_window_finished_at_ms,
        recommended_actions: impact.recommended_actions,
    }))
}

async fn create_connector(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<CreateConnectorRequest>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;

    let connector_id = validate_enterprise_id("connector_id", &input.connector_id)?;
    let provider = validate_enterprise_id("provider", &input.provider)?;
    let actor_id = request_principal
        .actor_id
        .clone()
        .unwrap_or_else(|| request_principal.source.clone());
    let mut connector = ConnectorInstance::active(
        connector_id,
        tenant_context.clone(),
        provider,
        PrincipalRef::human_user(actor_id),
        now_ms(),
    )
    .with_state(input.state, now_ms());
    connector.display_name = normalized_optional_label(input.display_name);

    {
        let mut registry = state.enterprise_connectors.write().await;
        registry.insert(enterprise_connector_key(&connector), connector);
        persist_enterprise_connectors(&state.enterprise_connectors_path, &registry).await?;
    }
    emit_connector_invalidation_required(
        &state,
        &tenant_context,
        &input.connector_id,
        "connector_created",
    );

    Ok(Json(EnterpriseAdminResponseBase {
        message: "enterprise connector saved",
        ..storage_base(tenant_context, request_principal)
    }))
}

async fn update_connector(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<UpdateConnectorRequest>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let connector_id = validate_enterprise_id("connector_id", &connector_id)?;

    let updated_connector = {
        let mut registry = state.enterprise_connectors.write().await;
        let Some(connector) = registry.values_mut().find(|connector| {
            connector.connector_id == connector_id && connector.tenant_matches(&tenant_context)
        }) else {
            return Err(not_found("ENTERPRISE_CONNECTOR_NOT_FOUND"));
        };
        if let Some(state) = input.state {
            connector.state = state;
        }
        if let Some(display_name) = input.display_name {
            connector.display_name = normalized_optional_label(Some(display_name));
        }
        connector.updated_at_ms = now_ms();
        let updated_connector = connector.clone();
        persist_enterprise_connectors(&state.enterprise_connectors_path, &registry).await?;
        updated_connector
    };
    emit_connector_invalidation_required(
        &state,
        &tenant_context,
        &updated_connector.connector_id,
        "connector_updated",
    );
    let _ = invalidate_response_cache_for_connector(
        &state,
        &tenant_context,
        &updated_connector.connector_id,
    )
    .await?;

    Ok(Json(EnterpriseAdminResponseBase {
        message: "enterprise connector updated",
        ..storage_base(tenant_context, request_principal)
    }))
}

async fn create_connector_credential_ref(
    State(state): State<AppState>,
    Path(connector_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<CreateConnectorCredentialRefRequest>,
) -> EnterpriseResult<EnterpriseConnectorsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    reject_raw_credential_value(input.credential_value.as_ref())?;

    let connector_id = validate_enterprise_id("connector_id", &connector_id)?;
    let credential_id = validate_enterprise_id("credential_id", &input.credential_id)?;
    let secret_ref = normalize_secret_ref_for_tenant(&input.secret_ref, &tenant_context)?;
    if let Some(resource_ref) = input.source_bound_resource.as_ref() {
        validate_resource_ref_matches_tenant(resource_ref, &tenant_context)?;
    }

    let updated_connector = {
        let mut registry = state.enterprise_connectors.write().await;
        let Some(connector) = registry.values_mut().find(|connector| {
            connector.connector_id == connector_id && connector.tenant_matches(&tenant_context)
        }) else {
            return Err(not_found("ENTERPRISE_CONNECTOR_NOT_FOUND"));
        };
        if connector
            .credential_refs
            .iter()
            .any(|credential| credential.credential_id == credential_id)
        {
            return Err(bad_request(
                "ENTERPRISE_CONNECTOR_CREDENTIAL_ALREADY_EXISTS",
            ));
        }
        validate_connector_credential_policy(
            connector,
            input.credential_class,
            input.source_bound_resource.as_ref(),
        )?;
        let now = now_ms();
        let credential_ref = ConnectorCredentialRef {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            connector_id: connector.connector_id.clone(),
            credential_id,
            credential_class: input.credential_class,
            secret_ref,
            source_bound_resource: input.source_bound_resource,
            created_at_ms: now,
            rotated_at_ms: None,
            expires_at_ms: input.expires_at_ms,
        };
        credential_ref
            .validate_for_tenant(&tenant_context)
            .map_err(|_| bad_request("ENTERPRISE_CONNECTOR_CREDENTIAL_TENANT_MISMATCH"))?;
        connector.credential_refs.push(credential_ref);
        connector.updated_at_ms = now;
        let updated_connector = connector.clone();
        persist_enterprise_connectors(&state.enterprise_connectors_path, &registry).await?;
        updated_connector
    };
    emit_connector_invalidation_required(
        &state,
        &tenant_context,
        &updated_connector.connector_id,
        "connector_credential_ref_created",
    );
    let _ = invalidate_response_cache_for_connector(
        &state,
        &tenant_context,
        &updated_connector.connector_id,
    )
    .await?;

    Ok(Json(EnterpriseConnectorsResponse {
        count: 1,
        connectors: vec![updated_connector],
        base: storage_base(tenant_context, request_principal),
    }))
}

async fn rotate_connector_credential_ref(
    State(state): State<AppState>,
    Path((connector_id, credential_id)): Path<(String, String)>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<RotateConnectorCredentialRefRequest>,
) -> EnterpriseResult<EnterpriseConnectorsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    reject_raw_credential_value(input.credential_value.as_ref())?;

    let connector_id = validate_enterprise_id("connector_id", &connector_id)?;
    let credential_id = validate_enterprise_id("credential_id", &credential_id)?;
    let secret_ref = normalize_secret_ref_for_tenant(&input.secret_ref, &tenant_context)?;

    let updated_connector = {
        let mut registry = state.enterprise_connectors.write().await;
        let Some(connector) = registry.values_mut().find(|connector| {
            connector.connector_id == connector_id && connector.tenant_matches(&tenant_context)
        }) else {
            return Err(not_found("ENTERPRISE_CONNECTOR_NOT_FOUND"));
        };
        let now = now_ms();
        let Some(credential_ref) = connector
            .credential_refs
            .iter_mut()
            .find(|credential| credential.credential_id == credential_id)
        else {
            return Err(not_found("ENTERPRISE_CONNECTOR_CREDENTIAL_NOT_FOUND"));
        };
        credential_ref.secret_ref = secret_ref;
        credential_ref.rotated_at_ms = Some(now);
        credential_ref.expires_at_ms = input.expires_at_ms;
        credential_ref
            .validate_for_tenant(&tenant_context)
            .map_err(|_| bad_request("ENTERPRISE_CONNECTOR_CREDENTIAL_TENANT_MISMATCH"))?;
        connector.updated_at_ms = now;
        let updated_connector = connector.clone();
        persist_enterprise_connectors(&state.enterprise_connectors_path, &registry).await?;
        updated_connector
    };
    emit_connector_invalidation_required(
        &state,
        &tenant_context,
        &updated_connector.connector_id,
        "connector_credential_ref_rotated",
    );
    let _ = invalidate_response_cache_for_connector(
        &state,
        &tenant_context,
        &updated_connector.connector_id,
    )
    .await?;

    Ok(Json(EnterpriseConnectorsResponse {
        count: 1,
        connectors: vec![updated_connector],
        base: storage_base(tenant_context, request_principal),
    }))
}

async fn update_source_binding(
    State(state): State<AppState>,
    Path(binding_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<UpdateSourceBindingRequest>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = validate_enterprise_id("binding_id", &binding_id)?;

    let updated_binding = {
        let mut registry = state.enterprise_source_bindings.write().await;
        let Some(binding) = registry.values_mut().find(|binding| {
            binding.binding_id == binding_id && binding.tenant_matches(&tenant_context)
        }) else {
            return Err(not_found("ENTERPRISE_SOURCE_BINDING_NOT_FOUND"));
        };
        if let Some(state) = input.state {
            binding.state = state;
        }
        if let Some(label) = input.source_root_label {
            binding.source_root_label = normalized_optional_label(Some(label));
        }
        if let Some(credential_ref_id) = input.credential_ref_id {
            binding.credential_ref_id = Some(validate_enterprise_id(
                "credential_ref_id",
                &credential_ref_id,
            )?);
        }
        if let Some(ingestion_policy) = input.ingestion_policy {
            binding.ingestion_policy = ingestion_policy;
        }
        binding.updated_at_ms = now_ms();
        let updated_binding = binding.clone();
        persist_enterprise_source_bindings(&state.enterprise_source_bindings_path, &registry)
            .await?;
        updated_binding
    };
    emit_source_binding_cache_invalidation_required(
        &state,
        &tenant_context,
        &updated_binding.binding_id,
        "source_binding_updated",
    );
    let _ = invalidate_response_cache_for_source_binding(
        &state,
        &tenant_context,
        &updated_binding.binding_id,
    )
    .await?;
    if source_binding_update_requires_index_purge(&updated_binding) {
        let db = open_enterprise_memory_db_for_state(&state).await?;
        let tenant_scope = memory_tenant_scope(&tenant_context);
        let _ = purge_source_binding_indexed_content(
            &db,
            &tenant_scope,
            &updated_binding.binding_id,
            source_object_state_for_binding_update(&updated_binding),
        )
        .await?;
    }

    Ok(Json(EnterpriseAdminResponseBase {
        message: "enterprise source binding updated",
        ..storage_base(tenant_context, request_principal)
    }))
}

pub(super) fn emit_source_binding_cache_invalidation_required(
    state: &AppState,
    tenant_context: &TenantContext,
    binding_id: &str,
    reason: &str,
) {
    state.event_bus.publish(EngineEvent::new(
        "enterprise.source_binding.cache_invalidation_required",
        json!({
            "reason": reason,
            "tenant_context": tenant_context,
            "binding_id": binding_id,
            "cache_scope": {
                "tenant_org_id": tenant_context.org_id,
                "tenant_workspace_id": tenant_context.workspace_id,
                "tenant_deployment_id": tenant_context.deployment_id,
                "source_binding_id": binding_id,
            }
        }),
    ));
}

fn emit_connector_invalidation_required(
    state: &AppState,
    tenant_context: &TenantContext,
    connector_id: &str,
    reason: &str,
) {
    state.event_bus.publish(EngineEvent::new(
        "enterprise.connector.cache_invalidation_required",
        json!({
            "reason": reason,
            "tenant_context": tenant_context,
            "connector_id": connector_id,
            "cache_scope": {
                "tenant_org_id": tenant_context.org_id,
                "tenant_workspace_id": tenant_context.workspace_id,
                "tenant_deployment_id": tenant_context.deployment_id,
                "connector_id": connector_id,
            }
        }),
    ));
}

async fn invalidate_response_cache_for_connector(
    state: &AppState,
    tenant_context: &TenantContext,
    connector_id: &str,
) -> Result<usize, (StatusCode, Json<Value>)> {
    let binding_ids: Vec<String> = state
        .enterprise_source_bindings
        .read()
        .await
        .values()
        .filter(|binding| {
            binding.connector_id == connector_id && binding.tenant_matches(tenant_context)
        })
        .map(|binding| binding.binding_id.clone())
        .collect();
    let mut removed = 0;
    for binding_id in binding_ids {
        removed += invalidate_response_cache_for_source_binding(state, tenant_context, &binding_id)
            .await?;
    }
    Ok(removed)
}

pub(super) async fn invalidate_response_cache_for_source_binding(
    state: &AppState,
    tenant_context: &TenantContext,
    binding_id: &str,
) -> Result<usize, (StatusCode, Json<Value>)> {
    let Some(parent) = state.memory_db_path.parent() else {
        return Ok(0);
    };
    if !parent.join("response_cache.db").exists() {
        return Ok(0);
    }
    let cache = ResponseCache::new(parent, 60, 10_000)
        .await
        .map_err(|_| internal_error("ENTERPRISE_RESPONSE_CACHE_INVALIDATION_FAILED"))?;
    cache
        .invalidate_source_binding(
            &tenant_context.org_id,
            &tenant_context.workspace_id,
            tenant_context.deployment_id.as_deref(),
            binding_id,
        )
        .await
        .map_err(|_| internal_error("ENTERPRISE_RESPONSE_CACHE_INVALIDATION_FAILED"))
}

pub(super) async fn purge_source_object_indexed_content(
    db: &MemoryDatabase,
    record: &SourceObjectLifecycleRecord,
) -> Result<(i64, i64), (StatusCode, Json<Value>)> {
    let result = db
        .delete_file_chunks_by_path_for_tenant(
            record.tier,
            record.session_id.as_deref(),
            record.project_id.as_deref(),
            &record.indexed_path,
            &record.tenant_scope,
        )
        .await
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_OBJECT_PURGE_FAILED"))?;
    db.delete_import_index_entry_for_tenant(
        record.tier,
        record.session_id.as_deref(),
        record.project_id.as_deref(),
        &record.indexed_path,
        &record.tenant_scope,
    )
    .await
    .map_err(|_| internal_error("ENTERPRISE_SOURCE_OBJECT_PURGE_FAILED"))?;
    Ok(result)
}

async fn purge_source_binding_indexed_content(
    db: &MemoryDatabase,
    tenant_scope: &MemoryTenantScope,
    binding_id: &str,
    lifecycle_state: SourceObjectLifecycleState,
) -> Result<(i64, i64), (StatusCode, Json<Value>)> {
    let source_objects = db
        .list_source_object_lifecycle_for_binding_for_tenant(tenant_scope, binding_id)
        .await
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_BINDING_PURGE_FAILED"))?;
    let mut chunks_deleted = 0;
    let mut bytes_estimated = 0;
    for record in source_objects {
        let (deleted, bytes) = purge_source_object_indexed_content(db, &record).await?;
        chunks_deleted += deleted;
        bytes_estimated += bytes;
        db.mark_source_object_lifecycle_state_for_tenant(
            tenant_scope,
            binding_id,
            &record.source_object_id,
            lifecycle_state,
            now_ms(),
        )
        .await
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_BINDING_PURGE_FAILED"))?;
    }
    Ok((chunks_deleted, bytes_estimated))
}

fn source_binding_update_requires_index_purge(binding: &SourceBinding) -> bool {
    !binding.state.allows_ingestion()
        || !binding.ingestion_policy.allow_indexing
        || !binding.ingestion_policy.allow_prompt_context
}

fn source_object_state_for_binding_update(binding: &SourceBinding) -> SourceObjectLifecycleState {
    if matches!(binding.state, SourceBindingState::Quarantined) {
        SourceObjectLifecycleState::Quarantined
    } else {
        SourceObjectLifecycleState::Tombstoned
    }
}

pub(super) async fn open_enterprise_memory_db() -> Result<MemoryDatabase, (StatusCode, Json<Value>)>
{
    let paths = tandem_core::resolve_shared_paths()
        .map_err(|_| internal_error("ENTERPRISE_MEMORY_DB_OPEN_FAILED"))?;
    if let Some(parent) = paths.memory_db_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_MEMORY_DB_OPEN_FAILED"))?;
    }
    MemoryDatabase::new(&paths.memory_db_path)
        .await
        .map_err(|_| internal_error("ENTERPRISE_MEMORY_DB_OPEN_FAILED"))
}

pub(super) async fn open_enterprise_memory_db_for_state(
    state: &AppState,
) -> Result<MemoryDatabase, (StatusCode, Json<Value>)> {
    if let Some(parent) = state.memory_db_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_MEMORY_DB_OPEN_FAILED"))?;
    }
    MemoryDatabase::new(&state.memory_db_path)
        .await
        .map_err(|_| internal_error("ENTERPRISE_MEMORY_DB_OPEN_FAILED"))
}

pub(super) fn memory_tenant_scope(tenant_context: &TenantContext) -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        deployment_id: tenant_context.deployment_id.clone(),
    }
}

struct ConnectorImpact {
    affected_bindings: Vec<SourceBinding>,
    affected_source_objects: Vec<SourceObjectLifecycleRecord>,
    affected_ingestion_jobs: Vec<IngestionJob>,
    affected_quarantines: Vec<IngestionQuarantine>,
    cache_invalidation_required: bool,
    compromise_window_started_at_ms: Option<u64>,
    compromise_window_finished_at_ms: Option<u64>,
    recommended_actions: Vec<&'static str>,
}

pub(super) async fn connector_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
    connector_id: &str,
) -> Result<ConnectorInstance, (StatusCode, Json<Value>)> {
    state
        .enterprise_connectors
        .read()
        .await
        .values()
        .find(|connector| {
            connector.connector_id == connector_id && connector.tenant_matches(tenant_context)
        })
        .cloned()
        .ok_or_else(|| not_found("ENTERPRISE_CONNECTOR_NOT_FOUND"))
}

pub(super) async fn source_binding_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
    binding_id: &str,
) -> Result<SourceBinding, (StatusCode, Json<Value>)> {
    state
        .enterprise_source_bindings
        .read()
        .await
        .values()
        .find(|binding| binding.binding_id == binding_id && binding.tenant_matches(tenant_context))
        .cloned()
        .ok_or_else(|| not_found("ENTERPRISE_SOURCE_BINDING_NOT_FOUND"))
}

async fn ensure_connector_exists_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
    connector_id: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let registry = state.enterprise_connectors.read().await;
    if registry.values().any(|connector| {
        connector.connector_id == connector_id && connector.tenant_matches(tenant_context)
    }) {
        Ok(())
    } else {
        Err(not_found("ENTERPRISE_CONNECTOR_NOT_FOUND"))
    }
}

async fn build_connector_impact(
    state: &AppState,
    tenant_context: &TenantContext,
    connector_id: &str,
) -> Result<ConnectorImpact, (StatusCode, Json<Value>)> {
    let mut affected_bindings: Vec<_> = state
        .enterprise_source_bindings
        .read()
        .await
        .values()
        .filter(|binding| {
            binding.connector_id == connector_id && binding.tenant_matches(tenant_context)
        })
        .cloned()
        .collect();
    affected_bindings.sort_by(|left, right| left.binding_id.cmp(&right.binding_id));

    let tenant_scope = memory_tenant_scope(tenant_context);
    let db = open_enterprise_memory_db_for_state(state).await?;
    let mut affected_source_objects = Vec::new();
    for binding in &affected_bindings {
        let mut rows = db
            .list_source_object_lifecycle_for_binding_for_tenant(&tenant_scope, &binding.binding_id)
            .await
            .map_err(|_| internal_error("ENTERPRISE_CONNECTOR_IMPACT_SOURCE_OBJECTS_FAILED"))?;
        affected_source_objects.append(&mut rows);
    }
    affected_source_objects.sort_by(|left, right| {
        left.source_binding_id
            .cmp(&right.source_binding_id)
            .then_with(|| left.source_object_id.cmp(&right.source_object_id))
    });

    let mut affected_ingestion_jobs: Vec<_> = state
        .enterprise_ingestion_jobs
        .read()
        .await
        .values()
        .filter(|job| {
            job.connector_id == connector_id && ingestion_job_tenant_matches(job, tenant_context)
        })
        .cloned()
        .collect();
    affected_ingestion_jobs.sort_by(|left, right| {
        right
            .started_at_ms
            .unwrap_or_default()
            .cmp(&left.started_at_ms.unwrap_or_default())
    });

    let mut affected_quarantines: Vec<_> = state
        .enterprise_ingestion_quarantines
        .read()
        .await
        .values()
        .filter(|quarantine| {
            quarantine.connector_id == connector_id
                && ingestion_quarantine_tenant_matches(quarantine, tenant_context)
        })
        .cloned()
        .collect();
    affected_quarantines.sort_by(|left, right| right.created_at_ms.cmp(&left.created_at_ms));

    let started = affected_source_objects
        .iter()
        .map(|source_object| source_object.first_seen_at_ms)
        .chain(
            affected_ingestion_jobs
                .iter()
                .filter_map(|job| job.started_at_ms),
        )
        .chain(
            affected_quarantines
                .iter()
                .map(|quarantine| quarantine.created_at_ms),
        )
        .min();
    let finished = affected_source_objects
        .iter()
        .map(|source_object| source_object.last_seen_at_ms)
        .chain(
            affected_source_objects
                .iter()
                .filter_map(|source_object| source_object.tombstoned_at_ms),
        )
        .chain(
            affected_ingestion_jobs
                .iter()
                .filter_map(|job| job.finished_at_ms.or(job.started_at_ms)),
        )
        .chain(affected_quarantines.iter().map(|quarantine| {
            quarantine
                .reviewed_at_ms
                .unwrap_or(quarantine.created_at_ms)
        }))
        .max();
    let cache_invalidation_required = !affected_bindings.is_empty()
        || !affected_source_objects.is_empty()
        || !affected_ingestion_jobs.is_empty()
        || !affected_quarantines.is_empty();

    Ok(ConnectorImpact {
        affected_bindings,
        affected_source_objects,
        affected_ingestion_jobs,
        affected_quarantines,
        cache_invalidation_required,
        compromise_window_started_at_ms: started,
        compromise_window_finished_at_ms: finished,
        recommended_actions: vec![
            "pause_or_revoke_connector",
            "invalidate_response_cache",
            "audit_compromise_window",
            "review_quarantine_records",
            "reindex_or_delete_affected_source_objects",
            "rotate_connector_credential",
        ],
    })
}

pub(super) fn serialize_data_class(
    data_class: DataClass,
) -> Result<String, (StatusCode, Json<Value>)> {
    serde_json::to_value(data_class)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .ok_or_else(|| internal_error("ENTERPRISE_DATA_CLASS_SERIALIZE_FAILED"))
}

async fn ensure_source_binding_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
    binding_id: &str,
) -> Result<SourceBinding, (StatusCode, Json<Value>)> {
    state
        .enterprise_source_bindings
        .read()
        .await
        .values()
        .find(|binding| binding.binding_id == binding_id && binding.tenant_matches(tenant_context))
        .cloned()
        .ok_or_else(|| not_found("ENTERPRISE_SOURCE_BINDING_NOT_FOUND"))
}

pub(super) fn storage_base(
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
) -> EnterpriseAdminResponseBase {
    EnterpriseAdminResponseBase {
        tenant_context,
        request_principal,
        bridge_state: "storage_backed",
        status: "ok",
        message: "enterprise admin storage is configured",
    }
}

fn noop_base(
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
) -> EnterpriseAdminResponseBase {
    EnterpriseAdminResponseBase {
        tenant_context,
        request_principal,
        bridge_state: "absent",
        status: "noop",
        message: "enterprise admin storage is not configured",
    }
}

pub(super) fn require_enterprise_admin(
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if enterprise_admin_allowed_for_mutation(request_principal, verified_tenant_context) {
        return Ok(());
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(json!({
            "code": "ENTERPRISE_ADMIN_REQUIRED",
            "message": "enterprise admin access is required for this mutation"
        })),
    ))
}

pub(super) fn enterprise_admin_allowed_for_mutation(
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> bool {
    if let Some(verified) = verified_tenant_context {
        return verified
            .roles
            .iter()
            .any(|role| is_enterprise_admin_role(role));
    }

    matches!(
        request_principal.source.as_str(),
        "api_token" | "control_panel" | "local_api_token" | "local_control_panel"
    )
}

fn is_enterprise_admin_role(role: &str) -> bool {
    matches!(
        role.trim().to_ascii_lowercase().as_str(),
        "admin"
            | "owner"
            | "org:admin"
            | "organization:admin"
            | "workspace:admin"
            | "enterprise:admin"
            | "reconfigure"
    )
}

pub(super) fn validate_enterprise_id(
    field: &str,
    value: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
    let value = value.trim();
    if value.is_empty() || value.len() > 96 {
        return Err(bad_request(format!("ENTERPRISE_{field}_INVALID")));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(bad_request(format!("ENTERPRISE_{field}_INVALID")));
    }
    Ok(value.to_string())
}

pub(super) fn validate_external_id(
    field: &str,
    value: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
    let value = value.trim();
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(bad_request(format!("ENTERPRISE_{field}_INVALID")));
    }
    Ok(value.to_string())
}

pub(super) fn validate_resource_ref_matches_tenant(
    resource_ref: &ResourceRef,
    tenant_context: &TenantContext,
) -> Result<(), (StatusCode, Json<Value>)> {
    if resource_ref.organization_id != tenant_context.org_id
        || resource_ref.workspace_id != tenant_context.workspace_id
    {
        return Err(bad_request(
            "ENTERPRISE_SOURCE_BINDING_RESOURCE_TENANT_MISMATCH",
        ));
    }
    Ok(())
}

fn normalize_secret_ref_for_tenant(
    secret_ref: &SecretRef,
    tenant_context: &TenantContext,
) -> Result<SecretRef, (StatusCode, Json<Value>)> {
    if secret_ref.org_id != tenant_context.org_id
        || secret_ref.workspace_id != tenant_context.workspace_id
    {
        return Err(bad_request(
            "ENTERPRISE_CONNECTOR_CREDENTIAL_TENANT_MISMATCH",
        ));
    }
    let provider = validate_enterprise_id("secret_provider", &secret_ref.provider)?;
    let secret_id = validate_external_id("secret_id", &secret_ref.secret_id)?;
    let name = normalized_optional_label(Some(secret_ref.name.clone()))
        .ok_or_else(|| bad_request("ENTERPRISE_SECRET_NAME_INVALID"))?;
    Ok(SecretRef {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        provider,
        secret_id,
        name,
    })
}

fn reject_raw_credential_value(value: Option<&Value>) -> Result<(), (StatusCode, Json<Value>)> {
    if value.is_some() {
        return Err(bad_request(
            "ENTERPRISE_CONNECTOR_CREDENTIAL_VALUE_NOT_ALLOWED",
        ));
    }
    Ok(())
}

fn validate_google_drive_source_binding_policy(
    connector_id: &str,
    source_type: &str,
    ingestion_policy: &IngestionPolicy,
) -> Result<(), (StatusCode, Json<Value>)> {
    if connector_id != GOOGLE_DRIVE_PROVIDER && source_type != GOOGLE_DRIVE_SOURCE_TYPE {
        return Ok(());
    }
    if source_type != GOOGLE_DRIVE_SOURCE_TYPE {
        return Err(bad_request("ENTERPRISE_GOOGLE_DRIVE_SOURCE_TYPE_REQUIRED"));
    }
    if !ingestion_policy.allow_prompt_context && ingestion_policy.allow_indexing {
        return Err(bad_request(
            "ENTERPRISE_GOOGLE_DRIVE_INDEXING_REQUIRES_PROMPT_CONTEXT_POLICY",
        ));
    }
    Ok(())
}

fn validate_connector_credential_policy(
    connector: &ConnectorInstance,
    credential_class: ConnectorCredentialClass,
    source_bound_resource: Option<&ResourceRef>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if connector.provider != GOOGLE_DRIVE_PROVIDER {
        return Ok(());
    }
    if credential_class != ConnectorCredentialClass::ReadOnly {
        return Err(bad_request(
            "ENTERPRISE_GOOGLE_DRIVE_READ_ONLY_CREDENTIAL_REQUIRED",
        ));
    }
    if source_bound_resource.is_none() {
        return Err(bad_request(
            "ENTERPRISE_GOOGLE_DRIVE_SOURCE_BOUND_CREDENTIAL_REQUIRED",
        ));
    }
    Ok(())
}

fn normalized_optional_label(label: Option<String>) -> Option<String> {
    label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn bad_request(code: impl Into<String>) -> (StatusCode, Json<Value>) {
    let code = code.into();
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "code": code,
            "message": "enterprise request validation failed"
        })),
    )
}

fn enterprise_source_binding_key(binding: &SourceBinding) -> String {
    let deployment = binding
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        binding.tenant_context.org_id,
        binding.tenant_context.workspace_id,
        deployment,
        binding.binding_id
    )
}

fn enterprise_connector_key(connector: &ConnectorInstance) -> String {
    let deployment = connector
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        connector.tenant_context.org_id,
        connector.tenant_context.workspace_id,
        deployment,
        connector.connector_id
    )
}

pub(super) fn enterprise_ingestion_job_key(job: &IngestionJob) -> String {
    let deployment = job
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        job.tenant_context.org_id, job.tenant_context.workspace_id, deployment, job.job_id
    )
}

pub(super) fn ingestion_job_tenant_matches(
    job: &IngestionJob,
    tenant_context: &TenantContext,
) -> bool {
    job.tenant_context.org_id == tenant_context.org_id
        && job.tenant_context.workspace_id == tenant_context.workspace_id
        && job.tenant_context.deployment_id == tenant_context.deployment_id
}

pub(super) fn ingestion_quarantine_tenant_matches(
    quarantine: &IngestionQuarantine,
    tenant_context: &TenantContext,
) -> bool {
    quarantine.tenant_context.org_id == tenant_context.org_id
        && quarantine.tenant_context.workspace_id == tenant_context.workspace_id
        && quarantine.tenant_context.deployment_id == tenant_context.deployment_id
}

async fn persist_enterprise_source_bindings(
    path: &std::path::Path,
    registry: &HashMap<String, SourceBinding>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_SOURCE_BINDINGS_PERSIST_FAILED"))?;
    }
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_BINDINGS_PERSIST_FAILED"))?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_BINDINGS_PERSIST_FAILED"))?;
    Ok(())
}

async fn persist_enterprise_connectors(
    path: &std::path::Path,
    registry: &HashMap<String, ConnectorInstance>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_CONNECTORS_PERSIST_FAILED"))?;
    }
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|_| internal_error("ENTERPRISE_CONNECTORS_PERSIST_FAILED"))?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| internal_error("ENTERPRISE_CONNECTORS_PERSIST_FAILED"))?;
    Ok(())
}

pub(super) async fn persist_enterprise_ingestion_jobs(
    path: &std::path::Path,
    registry: &HashMap<String, IngestionJob>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_INGESTION_JOBS_PERSIST_FAILED"))?;
    }
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|_| internal_error("ENTERPRISE_INGESTION_JOBS_PERSIST_FAILED"))?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| internal_error("ENTERPRISE_INGESTION_JOBS_PERSIST_FAILED"))?;
    Ok(())
}

pub(super) async fn persist_enterprise_ingestion_quarantines(
    path: &std::path::Path,
    registry: &HashMap<String, IngestionQuarantine>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_INGESTION_QUARANTINES_PERSIST_FAILED"))?;
    }
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|_| internal_error("ENTERPRISE_INGESTION_QUARANTINES_PERSIST_FAILED"))?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| internal_error("ENTERPRISE_INGESTION_QUARANTINES_PERSIST_FAILED"))?;
    Ok(())
}

pub(super) fn not_found(code: impl Into<String>) -> (StatusCode, Json<Value>) {
    let code = code.into();
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "code": code,
            "message": "enterprise resource was not found"
        })),
    )
}

pub(super) fn internal_error(code: impl Into<String>) -> (StatusCode, Json<Value>) {
    let code = code.into();
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "code": code,
            "message": "enterprise storage operation failed"
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_enterprise_contract::{AuthorityChain, HumanActor};

    fn verified_with_roles(roles: Vec<&str>) -> VerifiedTenantContext {
        let request_principal = RequestPrincipal::authenticated_user("user-a", "tandem-web");
        VerifiedTenantContext {
            tenant_context: TenantContext::explicit_user_workspace(
                "org-a",
                "workspace-a",
                Some("deployment-a".to_string()),
                "user-a",
            ),
            human_actor: HumanActor::tandem_user("user-a"),
            authority_chain: AuthorityChain::from_request(request_principal),
            roles: roles.into_iter().map(ToOwned::to_owned).collect(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 2_000,
            assertion_id: "assertion-a".to_string(),
        }
    }

    #[test]
    fn hosted_enterprise_mutations_require_signed_admin_role() {
        let local = RequestPrincipal::authenticated_user("user-a", "api_token");
        assert!(enterprise_admin_allowed_for_mutation(&local, None));

        let member = RequestPrincipal::authenticated_user("user-a", "tandem-web");
        assert!(!enterprise_admin_allowed_for_mutation(
            &member,
            Some(&verified_with_roles(vec!["member"]))
        ));
        assert!(enterprise_admin_allowed_for_mutation(
            &member,
            Some(&verified_with_roles(vec!["workspace:admin"]))
        ));
    }
}
