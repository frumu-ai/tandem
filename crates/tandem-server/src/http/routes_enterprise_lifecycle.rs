use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_enterprise_contract::{
    DataClass, IngestionJob, IngestionJobState, IngestionQuarantine, PrincipalRef,
    QuarantineDisposition, RequestPrincipal, ResourceRef, TenantContext, VerifiedTenantContext,
};
use tandem_memory::db::MemoryDatabase;
use tandem_memory::types::{MemoryTenantScope, SourceObjectLifecycleRecord};

use crate::{util::time::now_ms, AppState};

use super::routes_enterprise::{
    emit_source_binding_cache_invalidation_required, ingestion_job_tenant_matches,
    ingestion_quarantine_tenant_matches, internal_error,
    invalidate_response_cache_for_source_binding, memory_tenant_scope, not_found,
    open_enterprise_memory_db_for_state, persist_enterprise_ingestion_jobs,
    persist_enterprise_ingestion_quarantines, purge_source_object_indexed_content,
    require_enterprise_admin, serialize_data_class, source_binding_for_tenant, storage_base,
    validate_enterprise_id, validate_resource_ref_matches_tenant, EnterpriseAdminResponseBase,
    EnterpriseResult,
};

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseSourceObjectsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    source_objects: Vec<SourceObjectLifecycleRecord>,
    count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseSourceObjectActionResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    action: &'static str,
    source_object: Option<SourceObjectLifecycleRecord>,
    chunks_deleted: i64,
    bytes_estimated: i64,
    import_index_deleted: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseIngestionQuarantinesResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    quarantines: Vec<IngestionQuarantine>,
    count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseIngestionJobsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    ingestion_jobs: Vec<IngestionJob>,
    count: usize,
}

#[derive(Debug, Deserialize)]
pub(super) struct ListIngestionJobsQuery {
    #[serde(default)]
    binding_id: Option<String>,
    #[serde(default)]
    connector_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ListIngestionQuarantinesQuery {
    #[serde(default)]
    binding_id: Option<String>,
    #[serde(default)]
    connector_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReviewIngestionQuarantineRequest {
    disposition: QuarantineDisposition,
}

#[derive(Debug, Deserialize)]
pub(super) struct RescopeSourceObjectRequest {
    resource_ref: ResourceRef,
    data_class: DataClass,
}

pub(super) async fn list_ingestion_jobs(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<ListIngestionJobsQuery>,
) -> EnterpriseResult<EnterpriseIngestionJobsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = query
        .binding_id
        .as_deref()
        .and_then(|value| validate_enterprise_id("binding_id", value).ok());
    let connector_id = query
        .connector_id
        .as_deref()
        .and_then(|value| validate_enterprise_id("connector_id", value).ok());
    let mut ingestion_jobs: Vec<_> = state
        .enterprise_ingestion_jobs
        .read()
        .await
        .values()
        .filter(|job| ingestion_job_tenant_matches(job, &tenant_context))
        .filter(|job| {
            binding_id
                .as_ref()
                .is_none_or(|binding_id| job.binding_id == *binding_id)
        })
        .filter(|job| {
            connector_id
                .as_ref()
                .is_none_or(|connector_id| job.connector_id == *connector_id)
        })
        .cloned()
        .collect();
    ingestion_jobs.sort_by(|left, right| {
        right
            .started_at_ms
            .unwrap_or_default()
            .cmp(&left.started_at_ms.unwrap_or_default())
            .then_with(|| right.job_id.cmp(&left.job_id))
    });

    Ok(Json(EnterpriseIngestionJobsResponse {
        count: ingestion_jobs.len(),
        ingestion_jobs,
        base: storage_base(tenant_context, request_principal),
    }))
}

pub(super) async fn list_ingestion_quarantines(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<ListIngestionQuarantinesQuery>,
) -> EnterpriseResult<EnterpriseIngestionQuarantinesResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = query
        .binding_id
        .as_deref()
        .and_then(|value| validate_enterprise_id("binding_id", value).ok());
    let connector_id = query
        .connector_id
        .as_deref()
        .and_then(|value| validate_enterprise_id("connector_id", value).ok());
    let mut quarantines: Vec<_> = state
        .enterprise_ingestion_quarantines
        .read()
        .await
        .values()
        .filter(|quarantine| ingestion_quarantine_tenant_matches(quarantine, &tenant_context))
        .filter(|quarantine| {
            binding_id
                .as_ref()
                .is_none_or(|binding_id| quarantine.binding_id == *binding_id)
        })
        .filter(|quarantine| {
            connector_id
                .as_ref()
                .is_none_or(|connector_id| quarantine.connector_id == *connector_id)
        })
        .cloned()
        .collect();
    quarantines.sort_by(|left, right| {
        right
            .created_at_ms
            .cmp(&left.created_at_ms)
            .then_with(|| right.quarantine_id.cmp(&left.quarantine_id))
    });

    Ok(Json(EnterpriseIngestionQuarantinesResponse {
        count: quarantines.len(),
        quarantines,
        base: storage_base(tenant_context, request_principal),
    }))
}

pub(super) async fn review_ingestion_quarantine(
    State(state): State<AppState>,
    Path(quarantine_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<ReviewIngestionQuarantineRequest>,
) -> EnterpriseResult<EnterpriseIngestionQuarantinesResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let quarantine_id = validate_enterprise_id("quarantine_id", &quarantine_id)?;
    let actor_id = request_principal
        .actor_id
        .clone()
        .unwrap_or_else(|| request_principal.source.clone());
    let reviewed = {
        let mut registry = state.enterprise_ingestion_quarantines.write().await;
        let Some(quarantine) = registry.values_mut().find(|quarantine| {
            quarantine.quarantine_id == quarantine_id
                && ingestion_quarantine_tenant_matches(quarantine, &tenant_context)
        }) else {
            return Err(not_found("ENTERPRISE_INGESTION_QUARANTINE_NOT_FOUND"));
        };
        quarantine.disposition = Some(input.disposition);
        quarantine.reviewed_by = Some(PrincipalRef::human_user(actor_id));
        quarantine.reviewed_at_ms = Some(now_ms());
        let reviewed = quarantine.clone();
        persist_enterprise_ingestion_quarantines(
            &state.enterprise_ingestion_quarantines_path,
            &registry,
        )
        .await?;
        reviewed
    };

    update_ingestion_job_after_quarantine_review(&state, &tenant_context, &reviewed).await?;
    emit_source_binding_cache_invalidation_required(
        &state,
        &tenant_context,
        &reviewed.binding_id,
        "ingestion_quarantine_reviewed",
    );
    let _ =
        invalidate_response_cache_for_source_binding(&state, &tenant_context, &reviewed.binding_id)
            .await?;

    Ok(Json(EnterpriseIngestionQuarantinesResponse {
        count: 1,
        quarantines: vec![reviewed],
        base: storage_base(tenant_context, request_principal),
    }))
}

pub(super) async fn list_source_objects(
    State(state): State<AppState>,
    Path(binding_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseSourceObjectsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = validate_enterprise_id("binding_id", &binding_id)?;
    source_binding_for_tenant(&state, &tenant_context, &binding_id).await?;
    let db = open_enterprise_memory_db_for_state(&state).await?;
    let mut source_objects = db
        .list_source_object_lifecycle_for_binding_for_tenant(
            &memory_tenant_scope(&tenant_context),
            &binding_id,
        )
        .await
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_OBJECTS_LIST_FAILED"))?;
    source_objects.sort_by(|left, right| {
        left.resource_ref
            .to_string()
            .cmp(&right.resource_ref.to_string())
            .then_with(|| left.indexed_path.cmp(&right.indexed_path))
    });

    Ok(Json(EnterpriseSourceObjectsResponse {
        base: storage_base(tenant_context, request_principal),
        count: source_objects.len(),
        source_objects,
    }))
}

pub(super) async fn reindex_source_object(
    State(state): State<AppState>,
    Path((binding_id, source_object_id)): Path<(String, String)>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseSourceObjectActionResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = validate_enterprise_id("binding_id", &binding_id)?;
    let source_object_id = validate_enterprise_id("source_object_id", &source_object_id)?;
    source_binding_for_tenant(&state, &tenant_context, &binding_id).await?;
    let db = open_enterprise_memory_db_for_state(&state).await?;
    let tenant_scope = memory_tenant_scope(&tenant_context);
    let record = source_object_by_id(&db, &tenant_scope, &binding_id, &source_object_id).await?;
    let (chunks_deleted, bytes_estimated) =
        purge_source_object_indexed_content(&db, &record).await?;
    db.mark_source_object_lifecycle_state_for_tenant(
        &tenant_scope,
        &binding_id,
        &source_object_id,
        tandem_memory::types::SourceObjectLifecycleState::Active,
        now_ms(),
    )
    .await
    .map_err(|_| internal_error("ENTERPRISE_SOURCE_OBJECT_REINDEX_FAILED"))?;
    emit_source_binding_cache_invalidation_required(
        &state,
        &tenant_context,
        &binding_id,
        "source_object_reindex_requested",
    );
    let _ =
        invalidate_response_cache_for_source_binding(&state, &tenant_context, &binding_id).await?;
    let source_object = source_object_by_id(&db, &tenant_scope, &binding_id, &source_object_id)
        .await
        .ok();

    Ok(Json(EnterpriseSourceObjectActionResponse {
        base: storage_base(tenant_context, request_principal),
        action: "reindex_requested",
        source_object,
        chunks_deleted,
        bytes_estimated,
        import_index_deleted: true,
    }))
}

pub(super) async fn delete_source_object(
    State(state): State<AppState>,
    Path((binding_id, source_object_id)): Path<(String, String)>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseSourceObjectActionResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = validate_enterprise_id("binding_id", &binding_id)?;
    let source_object_id = validate_enterprise_id("source_object_id", &source_object_id)?;
    source_binding_for_tenant(&state, &tenant_context, &binding_id).await?;
    let db = open_enterprise_memory_db_for_state(&state).await?;
    let tenant_scope = memory_tenant_scope(&tenant_context);
    let record = source_object_by_id(&db, &tenant_scope, &binding_id, &source_object_id).await?;
    let (chunks_deleted, bytes_estimated) =
        purge_source_object_indexed_content(&db, &record).await?;
    db.delete_source_object_lifecycle_for_tenant(&tenant_scope, &binding_id, &source_object_id)
        .await
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_OBJECT_DELETE_FAILED"))?;
    emit_source_binding_cache_invalidation_required(
        &state,
        &tenant_context,
        &binding_id,
        "source_object_deleted",
    );
    let _ =
        invalidate_response_cache_for_source_binding(&state, &tenant_context, &binding_id).await?;

    Ok(Json(EnterpriseSourceObjectActionResponse {
        base: storage_base(tenant_context, request_principal),
        action: "deleted",
        source_object: Some(record),
        chunks_deleted,
        bytes_estimated,
        import_index_deleted: true,
    }))
}

pub(super) async fn rescope_source_object(
    State(state): State<AppState>,
    Path((binding_id, source_object_id)): Path<(String, String)>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<RescopeSourceObjectRequest>,
) -> EnterpriseResult<EnterpriseSourceObjectActionResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = validate_enterprise_id("binding_id", &binding_id)?;
    let source_object_id = validate_enterprise_id("source_object_id", &source_object_id)?;
    validate_resource_ref_matches_tenant(&input.resource_ref, &tenant_context)?;
    source_binding_for_tenant(&state, &tenant_context, &binding_id).await?;
    let db = open_enterprise_memory_db_for_state(&state).await?;
    let tenant_scope = memory_tenant_scope(&tenant_context);
    let record = source_object_by_id(&db, &tenant_scope, &binding_id, &source_object_id).await?;
    let (chunks_deleted, bytes_estimated) =
        purge_source_object_indexed_content(&db, &record).await?;
    let resource_ref = serde_json::to_value(input.resource_ref)
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_OBJECT_RESCOPE_FAILED"))?;
    let data_class = serialize_data_class(input.data_class)?;
    let updated = db
        .rescope_source_object_lifecycle_for_tenant(
            &tenant_scope,
            &binding_id,
            &source_object_id,
            &resource_ref,
            &data_class,
            now_ms(),
        )
        .await
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_OBJECT_RESCOPE_FAILED"))?;
    if !updated {
        return Err(not_found("ENTERPRISE_SOURCE_OBJECT_NOT_FOUND"));
    }
    emit_source_binding_cache_invalidation_required(
        &state,
        &tenant_context,
        &binding_id,
        "source_object_rescoped",
    );
    let _ =
        invalidate_response_cache_for_source_binding(&state, &tenant_context, &binding_id).await?;
    let source_object = source_object_by_id(&db, &tenant_scope, &binding_id, &source_object_id)
        .await
        .ok();

    Ok(Json(EnterpriseSourceObjectActionResponse {
        base: storage_base(tenant_context, request_principal),
        action: "rescoped",
        source_object,
        chunks_deleted,
        bytes_estimated,
        import_index_deleted: true,
    }))
}

async fn source_object_by_id(
    db: &MemoryDatabase,
    tenant_scope: &MemoryTenantScope,
    binding_id: &str,
    source_object_id: &str,
) -> Result<SourceObjectLifecycleRecord, (StatusCode, Json<Value>)> {
    db.get_source_object_lifecycle_by_id_for_tenant(tenant_scope, binding_id, source_object_id)
        .await
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_OBJECT_READ_FAILED"))?
        .ok_or_else(|| not_found("ENTERPRISE_SOURCE_OBJECT_NOT_FOUND"))
}

async fn update_ingestion_job_after_quarantine_review(
    state: &AppState,
    tenant_context: &TenantContext,
    quarantine: &IngestionQuarantine,
) -> Result<(), (StatusCode, Json<Value>)> {
    let mut registry = state.enterprise_ingestion_jobs.write().await;
    if let Some(job) = registry.values_mut().find(|job| {
        ingestion_job_tenant_matches(job, tenant_context)
            && job.quarantine_id.as_deref() == Some(quarantine.quarantine_id.as_str())
    }) {
        job.state = match quarantine.disposition {
            Some(QuarantineDisposition::Release) => IngestionJobState::Completed,
            Some(QuarantineDisposition::Delete) => IngestionJobState::Skipped,
            Some(QuarantineDisposition::Reindex) => IngestionJobState::Queued,
            None => job.state,
        };
        job.finished_at_ms = Some(now_ms());
        persist_enterprise_ingestion_jobs(&state.enterprise_ingestion_jobs_path, &registry).await?;
    }
    Ok(())
}
