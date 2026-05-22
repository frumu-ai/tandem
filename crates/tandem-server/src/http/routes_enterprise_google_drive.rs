use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    IngestionJob, IngestionJobState, IngestionQuarantine, RequestPrincipal, SourceBinding,
    TenantContext, VerifiedTenantContext,
};
use tandem_memory::import_files;
use tandem_memory::types::{
    MemoryImportFormat, MemoryImportProgress, MemoryImportRequest, MemoryImportSourceBinding,
    MemoryImportStats, MemoryTenantScope, MemoryTier, SourceObjectLifecycleRecord,
    SourceObjectLifecycleState,
};

use crate::enterprise_connectors::google_drive::GoogleDriveClient;
use crate::enterprise_connectors::google_drive_ingestion::{
    fetch_google_drive_binding_files, preflight_google_drive_binding, GoogleDriveBindingPreflight,
    GoogleDriveIngestionError,
};
use crate::enterprise_connectors::secrets::EnvSecretResolver;
use crate::{util::time::now_ms, AppState};

use super::routes_enterprise::{
    bad_request, connector_for_tenant, emit_source_binding_cache_invalidation_required,
    enterprise_ingestion_job_key, internal_error, invalidate_response_cache_for_source_binding,
    memory_tenant_scope, persist_enterprise_ingestion_jobs,
    persist_enterprise_ingestion_quarantines, require_enterprise_admin, serialize_data_class,
    source_binding_for_tenant, storage_base, validate_enterprise_id, EnterpriseAdminResponseBase,
    EnterpriseResult,
};

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseGoogleDrivePreflightResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    preflight: GoogleDriveBindingPreflight,
}

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseGoogleDriveImportResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    binding_id: String,
    connector_id: String,
    ingestion_job: IngestionJob,
    stats: MemoryImportStats,
    drive_files_fetched: usize,
    drive_files_skipped: usize,
}

#[derive(Debug, Deserialize)]
pub(super) struct EnterpriseGoogleDriveImportRequest {
    #[serde(default = "default_enterprise_connector_import_tier")]
    tier: MemoryTier,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    sync_deletes: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct EnterpriseGoogleDriveReindexRequest {
    #[serde(default = "default_enterprise_connector_import_tier")]
    tier: MemoryTier,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default = "default_google_drive_reindex_sync_deletes")]
    sync_deletes: bool,
    #[serde(default)]
    source_object_id: Option<String>,
}

fn default_enterprise_connector_import_tier() -> MemoryTier {
    MemoryTier::Global
}

fn default_google_drive_reindex_sync_deletes() -> bool {
    true
}

pub(super) async fn preflight_google_drive_source_binding(
    State(state): State<AppState>,
    Path(binding_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseGoogleDrivePreflightResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = validate_enterprise_id("binding_id", &binding_id)?;
    let binding = source_binding_for_tenant(&state, &tenant_context, &binding_id).await?;
    let connector = connector_for_tenant(&state, &tenant_context, &binding.connector_id).await?;
    let resolver = EnvSecretResolver;
    let drive_client = GoogleDriveClient::new_from_env();
    let preflight = preflight_google_drive_binding(
        &tenant_context,
        &connector,
        &binding,
        &resolver,
        &drive_client,
    )
    .await
    .map_err(map_google_drive_preflight_error)?;

    Ok(Json(EnterpriseGoogleDrivePreflightResponse {
        base: storage_base(tenant_context, request_principal),
        preflight,
    }))
}

pub(super) async fn import_google_drive_source_binding(
    State(state): State<AppState>,
    Path(binding_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<EnterpriseGoogleDriveImportRequest>,
) -> EnterpriseResult<EnterpriseGoogleDriveImportResponse> {
    let input = GoogleDriveImportOperationInput {
        tier: input.tier,
        project_id: input.project_id,
        session_id: input.session_id,
        sync_deletes: input.sync_deletes,
        source_object_id: None,
        job_kind: "import",
        empty_job_state: IngestionJobState::Completed,
        completion_reason: "google_drive_import_completed",
    };
    run_google_drive_import_operation(
        state,
        binding_id,
        tenant_context,
        request_principal,
        verified_tenant_context,
        input,
    )
    .await
}

pub(super) async fn reindex_google_drive_source_binding(
    State(state): State<AppState>,
    Path(binding_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<EnterpriseGoogleDriveReindexRequest>,
) -> EnterpriseResult<EnterpriseGoogleDriveImportResponse> {
    let source_object_id = input
        .source_object_id
        .map(|value| validate_enterprise_id("source_object_id", &value))
        .transpose()?;
    let input = GoogleDriveImportOperationInput {
        tier: input.tier,
        project_id: input.project_id,
        session_id: input.session_id,
        sync_deletes: input.sync_deletes,
        source_object_id,
        job_kind: "reindex",
        empty_job_state: IngestionJobState::Skipped,
        completion_reason: "google_drive_reindex_completed",
    };
    run_google_drive_import_operation(
        state,
        binding_id,
        tenant_context,
        request_principal,
        verified_tenant_context,
        input,
    )
    .await
}

struct GoogleDriveImportOperationInput {
    tier: MemoryTier,
    project_id: Option<String>,
    session_id: Option<String>,
    sync_deletes: bool,
    source_object_id: Option<String>,
    job_kind: &'static str,
    empty_job_state: IngestionJobState,
    completion_reason: &'static str,
}

async fn run_google_drive_import_operation(
    state: AppState,
    binding_id: String,
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    input: GoogleDriveImportOperationInput,
) -> EnterpriseResult<EnterpriseGoogleDriveImportResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = validate_enterprise_id("binding_id", &binding_id)?;
    match input.tier {
        MemoryTier::Project if input.project_id.as_deref().unwrap_or("").trim().is_empty() => {
            return Err(bad_request(
                "ENTERPRISE_GOOGLE_DRIVE_IMPORT_PROJECT_REQUIRED",
            ));
        }
        MemoryTier::Session if input.session_id.as_deref().unwrap_or("").trim().is_empty() => {
            return Err(bad_request(
                "ENTERPRISE_GOOGLE_DRIVE_IMPORT_SESSION_REQUIRED",
            ));
        }
        _ => {}
    }

    let binding = source_binding_for_tenant(&state, &tenant_context, &binding_id).await?;
    let connector = connector_for_tenant(&state, &tenant_context, &binding.connector_id).await?;
    if !binding.ingestion_policy.allow_indexing {
        return Err(bad_request(
            "ENTERPRISE_GOOGLE_DRIVE_IMPORT_INDEXING_DISABLED",
        ));
    }

    let resolver = EnvSecretResolver;
    let drive_client = GoogleDriveClient::new_from_env();
    let fetched = fetch_google_drive_binding_files(
        &tenant_context,
        &connector,
        &binding,
        &resolver,
        &drive_client,
    )
    .await
    .map_err(map_google_drive_import_error)?;

    let Some(memory_manager) = open_enterprise_memory_manager().await else {
        return Err(internal_error(
            "ENTERPRISE_GOOGLE_DRIVE_IMPORT_MEMORY_OPEN_FAILED",
        ));
    };
    let tenant_scope = memory_tenant_scope(&tenant_context);
    let source_object_filter = if let Some(source_object_id) = input.source_object_id.as_deref() {
        let record = memory_manager
            .db()
            .get_source_object_lifecycle_by_id_for_tenant(
                &tenant_scope,
                &binding.binding_id,
                source_object_id,
            )
            .await
            .map_err(|_| internal_error("ENTERPRISE_GOOGLE_DRIVE_REINDEX_SOURCE_OBJECT_FAILED"))?
            .ok_or_else(|| {
                bad_request("ENTERPRISE_GOOGLE_DRIVE_REINDEX_SOURCE_OBJECT_NOT_FOUND")
            })?;
        Some(record)
    } else {
        None
    };
    let mut fetched_files = fetched.files;
    if let Some(record) = source_object_filter.as_ref() {
        fetched_files.retain(|file| {
            google_drive_indexed_path(&binding.binding_id, &file.drive_file_id, &file.name)
                == record.indexed_path
        });
        if fetched_files.is_empty() {
            return Err(bad_request(
                "ENTERPRISE_GOOGLE_DRIVE_REINDEX_SOURCE_OBJECT_NOT_FETCHED",
            ));
        }
    }
    let effective_sync_deletes = input.sync_deletes && source_object_filter.is_none();

    if fetched_files.is_empty() && !effective_sync_deletes {
        let job_started_at_ms = now_ms();
        let completed_job = IngestionJob {
            job_id: format!(
                "google-drive-{}-{job_started_at_ms}-{}",
                input.job_kind,
                uuid::Uuid::new_v4()
            ),
            tenant_context: tenant_context.clone(),
            connector_id: binding.connector_id.clone(),
            binding_id: binding.binding_id.clone(),
            state: input.empty_job_state,
            source_object_ids: Vec::new(),
            started_at_ms: Some(job_started_at_ms),
            finished_at_ms: Some(now_ms()),
            quarantine_id: None,
        };
        record_enterprise_ingestion_job(&state, completed_job.clone()).await?;
        return Ok(Json(EnterpriseGoogleDriveImportResponse {
            base: storage_base(tenant_context, request_principal),
            binding_id: binding.binding_id,
            connector_id: connector.connector_id,
            ingestion_job: completed_job,
            stats: MemoryImportStats {
                discovered_files: fetched.skipped_files,
                skipped_files: fetched.skipped_files,
                ..MemoryImportStats::default()
            },
            drive_files_fetched: 0,
            drive_files_skipped: fetched.skipped_files,
        }));
    }

    let temp_dir = std::env::temp_dir().join(format!(
        "tandem-google-drive-{}-{binding_id}-{}",
        input.job_kind,
        uuid::Uuid::new_v4()
    ));
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .map_err(|_| internal_error("ENTERPRISE_GOOGLE_DRIVE_IMPORT_TEMP_FAILED"))?;
    for file in &fetched_files {
        let path = temp_dir.join(safe_google_drive_import_file_name(
            &file.drive_file_id,
            &file.name,
        ));
        tokio::fs::write(&path, &file.bytes)
            .await
            .map_err(|_| internal_error("ENTERPRISE_GOOGLE_DRIVE_IMPORT_TEMP_FAILED"))?;
    }

    let source_binding = memory_import_source_binding_from_enterprise(&binding)?;
    let job_started_at_ms = now_ms();
    let job_id = format!(
        "google-drive-{}-{job_started_at_ms}-{}",
        input.job_kind,
        uuid::Uuid::new_v4()
    );
    let running_job = IngestionJob {
        job_id: job_id.clone(),
        tenant_context: tenant_context.clone(),
        connector_id: binding.connector_id.clone(),
        binding_id: binding.binding_id.clone(),
        state: IngestionJobState::Running,
        source_object_ids: Vec::new(),
        started_at_ms: Some(job_started_at_ms),
        finished_at_ms: None,
        quarantine_id: None,
    };
    record_enterprise_ingestion_job(&state, running_job).await?;

    let import_request = MemoryImportRequest {
        root_path: temp_dir.display().to_string(),
        format: MemoryImportFormat::Directory,
        tier: input.tier,
        session_id: normalized_optional_id(input.session_id),
        project_id: normalized_optional_id(input.project_id),
        tenant_scope: tenant_scope.clone(),
        source_binding: Some(source_binding),
        sync_deletes: effective_sync_deletes,
        import_namespace: Some(format!("google-drive-{}", binding.binding_id)),
    };
    let stats = match import_files(
        &memory_manager,
        &import_request,
        None::<fn(&MemoryImportProgress)>,
    )
    .await
    {
        Ok(stats) => stats,
        Err(_) => {
            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
            return Err(internal_error(
                "ENTERPRISE_GOOGLE_DRIVE_IMPORT_INDEX_FAILED",
            ));
        }
    };

    let source_objects = source_objects_seen_since(
        &memory_manager,
        &tenant_scope,
        &binding.binding_id,
        job_started_at_ms,
    )
    .await
    .map_err(|_| internal_error("ENTERPRISE_GOOGLE_DRIVE_IMPORT_SOURCE_OBJECTS_FAILED"))?;
    let source_object_ids = source_objects
        .iter()
        .map(|record| record.source_object_id.clone())
        .collect::<Vec<_>>();
    let quarantine_id = if binding.ingestion_policy.require_review {
        let quarantine_id = format!("quarantine-{job_started_at_ms}-{}", uuid::Uuid::new_v4());
        quarantine_source_bound_import(
            &memory_manager,
            &tenant_scope,
            &binding.binding_id,
            &source_objects,
            job_started_at_ms,
        )
        .await
        .map_err(|_| internal_error("ENTERPRISE_GOOGLE_DRIVE_IMPORT_QUARANTINE_FAILED"))?;
        record_enterprise_ingestion_quarantine(
            &state,
            IngestionQuarantine {
                quarantine_id: quarantine_id.clone(),
                tenant_context: tenant_context.clone(),
                connector_id: binding.connector_id.clone(),
                binding_id: binding.binding_id.clone(),
                source_object_ids: source_object_ids.clone(),
                reason: "source binding requires ingestion review".to_string(),
                created_at_ms: now_ms(),
                reviewed_by: None,
                reviewed_at_ms: None,
                disposition: None,
            },
        )
        .await?;
        Some(quarantine_id)
    } else {
        None
    };
    let completed_job = IngestionJob {
        job_id,
        tenant_context: tenant_context.clone(),
        connector_id: binding.connector_id.clone(),
        binding_id: binding.binding_id.clone(),
        state: if binding.ingestion_policy.require_review {
            IngestionJobState::Quarantined
        } else if stats.errors > 0 {
            IngestionJobState::Failed
        } else {
            IngestionJobState::Completed
        },
        source_object_ids,
        started_at_ms: Some(job_started_at_ms),
        finished_at_ms: Some(now_ms()),
        quarantine_id,
    };
    record_enterprise_ingestion_job(&state, completed_job.clone()).await?;
    emit_source_binding_cache_invalidation_required(
        &state,
        &tenant_context,
        &binding.binding_id,
        input.completion_reason,
    );
    let _ =
        invalidate_response_cache_for_source_binding(&tenant_context, &binding.binding_id).await?;
    let _ = tokio::fs::remove_dir_all(&temp_dir).await;

    Ok(Json(EnterpriseGoogleDriveImportResponse {
        base: storage_base(tenant_context, request_principal),
        binding_id: binding.binding_id,
        connector_id: connector.connector_id,
        ingestion_job: completed_job,
        stats,
        drive_files_fetched: fetched_files.len(),
        drive_files_skipped: fetched.skipped_files,
    }))
}

fn map_google_drive_preflight_error(error: GoogleDriveIngestionError) -> (StatusCode, Json<Value>) {
    match error {
        GoogleDriveIngestionError::Secret(_) => {
            bad_request("ENTERPRISE_GOOGLE_DRIVE_SECRET_UNAVAILABLE")
        }
        GoogleDriveIngestionError::Drive(_) => {
            bad_request("ENTERPRISE_GOOGLE_DRIVE_PREFLIGHT_READ_FAILED")
        }
        GoogleDriveIngestionError::TenantMismatch
        | GoogleDriveIngestionError::ConnectorMismatch
        | GoogleDriveIngestionError::UnsupportedConnectorProvider
        | GoogleDriveIngestionError::UnsupportedSourceType
        | GoogleDriveIngestionError::ConnectorNotActive
        | GoogleDriveIngestionError::BindingNotEnabled
        | GoogleDriveIngestionError::MissingCredentialRef
        | GoogleDriveIngestionError::CredentialRefNotFound
        | GoogleDriveIngestionError::CredentialNotReadOnly
        | GoogleDriveIngestionError::CredentialResourceMismatch => {
            bad_request("ENTERPRISE_GOOGLE_DRIVE_PREFLIGHT_POLICY_FAILED")
        }
    }
}

fn map_google_drive_import_error(error: GoogleDriveIngestionError) -> (StatusCode, Json<Value>) {
    match error {
        GoogleDriveIngestionError::Secret(_) => {
            bad_request("ENTERPRISE_GOOGLE_DRIVE_SECRET_UNAVAILABLE")
        }
        GoogleDriveIngestionError::Drive(_) => {
            bad_request("ENTERPRISE_GOOGLE_DRIVE_IMPORT_READ_FAILED")
        }
        GoogleDriveIngestionError::TenantMismatch
        | GoogleDriveIngestionError::ConnectorMismatch
        | GoogleDriveIngestionError::UnsupportedConnectorProvider
        | GoogleDriveIngestionError::UnsupportedSourceType
        | GoogleDriveIngestionError::ConnectorNotActive
        | GoogleDriveIngestionError::BindingNotEnabled
        | GoogleDriveIngestionError::MissingCredentialRef
        | GoogleDriveIngestionError::CredentialRefNotFound
        | GoogleDriveIngestionError::CredentialNotReadOnly
        | GoogleDriveIngestionError::CredentialResourceMismatch => {
            bad_request("ENTERPRISE_GOOGLE_DRIVE_IMPORT_POLICY_FAILED")
        }
    }
}

async fn open_enterprise_memory_manager() -> Option<tandem_memory::MemoryManager> {
    let paths = tandem_core::resolve_shared_paths().ok()?;
    if let Some(parent) = paths.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    tandem_memory::MemoryManager::new(&paths.memory_db_path)
        .await
        .ok()
}

fn memory_import_source_binding_from_enterprise(
    binding: &SourceBinding,
) -> Result<MemoryImportSourceBinding, (StatusCode, Json<Value>)> {
    Ok(MemoryImportSourceBinding {
        binding_id: binding.binding_id.clone(),
        connector_id: binding.connector_id.clone(),
        resource_ref: serde_json::to_value(&binding.resource_ref)
            .map_err(|_| internal_error("ENTERPRISE_SOURCE_BINDING_RESOURCE_SERIALIZE_FAILED"))?,
        data_class: serialize_data_class(binding.data_class)?,
        require_review: binding.ingestion_policy.require_review,
    })
}

async fn record_enterprise_ingestion_job(
    state: &AppState,
    job: IngestionJob,
) -> Result<(), (StatusCode, Json<Value>)> {
    let mut registry = state.enterprise_ingestion_jobs.write().await;
    let key = enterprise_ingestion_job_key(&job);
    registry.insert(key, job);
    persist_enterprise_ingestion_jobs(&state.enterprise_ingestion_jobs_path, &registry).await
}

async fn record_enterprise_ingestion_quarantine(
    state: &AppState,
    quarantine: IngestionQuarantine,
) -> Result<(), (StatusCode, Json<Value>)> {
    let mut registry = state.enterprise_ingestion_quarantines.write().await;
    let deployment = quarantine
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    let key = format!(
        "{}::{}::{}::{}",
        quarantine.tenant_context.org_id,
        quarantine.tenant_context.workspace_id,
        deployment,
        quarantine.quarantine_id
    );
    registry.insert(key, quarantine);
    persist_enterprise_ingestion_quarantines(
        &state.enterprise_ingestion_quarantines_path,
        &registry,
    )
    .await
}

async fn source_objects_seen_since(
    manager: &tandem_memory::MemoryManager,
    tenant_scope: &MemoryTenantScope,
    binding_id: &str,
    started_at_ms: u64,
) -> Result<Vec<SourceObjectLifecycleRecord>, tandem_memory::types::MemoryError> {
    let mut records: Vec<_> = manager
        .db()
        .list_source_object_lifecycle_for_binding_for_tenant(tenant_scope, binding_id)
        .await?
        .into_iter()
        .filter(|record| record.last_seen_at_ms >= started_at_ms)
        .collect();
    records.sort_by(|left, right| left.source_object_id.cmp(&right.source_object_id));
    records.dedup_by(|left, right| left.source_object_id == right.source_object_id);
    Ok(records)
}

async fn quarantine_source_bound_import(
    manager: &tandem_memory::MemoryManager,
    tenant_scope: &MemoryTenantScope,
    binding_id: &str,
    source_objects: &[SourceObjectLifecycleRecord],
    changed_at_ms: u64,
) -> Result<(), tandem_memory::types::MemoryError> {
    for record in source_objects {
        manager
            .db()
            .delete_file_chunks_by_path_for_tenant(
                record.tier,
                record.session_id.as_deref(),
                record.project_id.as_deref(),
                &record.indexed_path,
                tenant_scope,
            )
            .await?;
        manager
            .db()
            .delete_import_index_entry_for_tenant(
                record.tier,
                record.session_id.as_deref(),
                record.project_id.as_deref(),
                &record.indexed_path,
                tenant_scope,
            )
            .await?;
        manager
            .db()
            .mark_source_object_lifecycle_state_for_tenant(
                tenant_scope,
                binding_id,
                &record.source_object_id,
                SourceObjectLifecycleState::Quarantined,
                changed_at_ms,
            )
            .await?;
    }
    Ok(())
}

fn safe_google_drive_import_file_name(file_id: &str, name: &str) -> String {
    let mut safe_id = sanitize_path_segment(file_id);
    if safe_id.is_empty() {
        safe_id = "drive-file".to_string();
    }
    let mut safe_name = sanitize_path_segment(name);
    if safe_name.is_empty() {
        safe_name = "document.txt".to_string();
    }
    if !safe_name.contains('.') {
        safe_name.push_str(".txt");
    }
    format!("{safe_id}-{safe_name}")
}

fn google_drive_indexed_path(binding_id: &str, file_id: &str, name: &str) -> String {
    format!(
        "google-drive-{binding_id}/{}",
        safe_google_drive_import_file_name(file_id, name)
    )
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(['.', '-'])
        .chars()
        .take(120)
        .collect()
}

fn normalized_optional_id(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
