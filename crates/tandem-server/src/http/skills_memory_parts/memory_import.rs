// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) async fn memory_import(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<MemoryImportInput>,
) -> Result<Json<MemoryImportResponse>, (StatusCode, Json<ErrorEnvelope>)> {
    let source_kind = input.source.kind.trim().to_ascii_lowercase();
    if source_kind != "path" {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source.kind must be `path`",
        ));
    }

    let path = input.source.path.trim().to_string();
    if path.is_empty() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source.path is required for path imports",
        ));
    }

    validate_memory_import_path(&path)?;

    let project_id = normalize_optional_memory_import_id(input.project_id);
    let session_id = normalize_optional_memory_import_id(input.session_id);
    let source_binding_id = normalize_optional_memory_import_id(input.source_binding_id);
    match input.tier {
        MemoryTier::Project if project_id.is_none() => {
            return Err(skill_error(
                StatusCode::BAD_REQUEST,
                "tier=project requires project_id",
            ));
        }
        MemoryTier::Session if session_id.is_none() => {
            return Err(skill_error(
                StatusCode::BAD_REQUEST,
                "tier=session requires session_id",
            ));
        }
        _ => {}
    }
    let source_binding = resolve_memory_import_source_binding(
        &state,
        &tenant_context,
        &request_principal,
        verified_tenant_context.as_deref(),
        source_binding_id.as_deref(),
    )
    .await?;
    let source_binding_for_job = source_binding.clone();
    let job_started_at_ms = crate::util::time::now_ms();
    let ingestion_job_id = source_binding_for_job.as_ref().map(|binding| {
        format!(
            "manual-import-{}-{}",
            job_started_at_ms,
            uuid::Uuid::new_v4()
        )
    });

    if let (Some(job_id), Some(binding)) = (&ingestion_job_id, source_binding_for_job.as_ref()) {
        if let Err(err) = record_enterprise_ingestion_job(
            &state,
            IngestionJob {
                job_id: job_id.clone(),
                tenant_context: tenant_context.clone(),
                connector_id: binding.connector_id.clone(),
                binding_id: binding.binding_id.clone(),
                state: IngestionJobState::Running,
                source_object_ids: Vec::new(),
                started_at_ms: Some(job_started_at_ms),
                finished_at_ms: None,
                quarantine_id: None,
            },
        )
        .await
        {
            tracing::warn!(
                error = %err,
                "failed to record enterprise ingestion job start"
            );
        }
    }

    publish_tenant_event(
        &state,
        &tenant_context,
        "memory.import.started",
        json!({
            "source": {"kind": "path", "path": path},
            "format": input.format,
            "tier": input.tier,
            "project_id": project_id.clone(),
            "session_id": session_id.clone(),
            "source_binding_id": source_binding_id.clone(),
            "sync_deletes": input.sync_deletes,
        }),
    );

    let Some(manager) = open_memory_manager_for_state(&state).await else {
        if let (Some(job_id), Some(binding)) = (&ingestion_job_id, source_binding_for_job.as_ref())
        {
            if let Err(err) = record_enterprise_ingestion_job(
                &state,
                IngestionJob {
                    job_id: job_id.clone(),
                    tenant_context: tenant_context.clone(),
                    connector_id: binding.connector_id.clone(),
                    binding_id: binding.binding_id.clone(),
                    state: IngestionJobState::Failed,
                    source_object_ids: Vec::new(),
                    started_at_ms: Some(job_started_at_ms),
                    finished_at_ms: Some(crate::util::time::now_ms()),
                    quarantine_id: None,
                },
            )
            .await
            {
                tracing::warn!(
                    error = %err,
                    "failed to record enterprise ingestion job failure"
                );
            }
        }
        publish_tenant_event(
            &state,
            &tenant_context,
            "memory.import.failed",
            json!({
                "source": {"kind": "path", "path": path},
                "format": input.format,
                "tier": input.tier,
                "source_binding_id": source_binding_id.clone(),
                "error": "failed to open memory manager",
            }),
        );
        return Err(skill_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to open memory manager",
        ));
    };

    let request = TandemMemoryImportRequest {
        root_path: path.clone(),
        format: input.format,
        tier: input.tier,
        session_id: session_id.clone(),
        project_id: project_id.clone(),
        tenant_scope: MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        },
        source_binding,
        sync_deletes: input.sync_deletes,
        import_namespace: None,
    };

    let stats = match with_verified_memory_decrypt_principal(
        verified_tenant_context.as_deref(),
        import_files(&manager, &request, None::<fn(&MemoryImportProgress)>),
    )
    .await
    {
        Ok(stats) => stats,
        Err(err) => {
            if let (Some(job_id), Some(binding)) =
                (&ingestion_job_id, source_binding_for_job.as_ref())
            {
                if let Err(record_err) = record_enterprise_ingestion_job(
                    &state,
                    IngestionJob {
                        job_id: job_id.clone(),
                        tenant_context: tenant_context.clone(),
                        connector_id: binding.connector_id.clone(),
                        binding_id: binding.binding_id.clone(),
                        state: IngestionJobState::Failed,
                        source_object_ids: Vec::new(),
                        started_at_ms: Some(job_started_at_ms),
                        finished_at_ms: Some(crate::util::time::now_ms()),
                        quarantine_id: None,
                    },
                )
                .await
                {
                    tracing::warn!(
                        error = %record_err,
                        "failed to record enterprise ingestion job failure"
                    );
                }
            }
            publish_tenant_event(
                &state,
                &tenant_context,
                "memory.import.failed",
                json!({
                    "source": {"kind": "path", "path": path},
                    "format": input.format,
                    "tier": input.tier,
                    "project_id": project_id.clone(),
                    "session_id": session_id.clone(),
                    "source_binding_id": source_binding_id.clone(),
                    "sync_deletes": input.sync_deletes,
                    "error": err.to_string(),
                }),
            );
            return Err(skill_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("memory import failed: {err}"),
            ));
        }
    };

    if let (Some(job_id), Some(binding)) = (&ingestion_job_id, source_binding_for_job.as_ref()) {
        let source_objects = with_verified_memory_decrypt_principal(
            verified_tenant_context.as_deref(),
            source_objects_seen_since(
                &manager,
                &request.tenant_scope,
                &binding.binding_id,
                job_started_at_ms,
            ),
        )
        .await
        .unwrap_or_default();
        let source_object_ids = source_objects
            .iter()
            .map(|record| record.source_object_id.clone())
            .collect::<Vec<_>>();
        let quarantine_id = if binding.require_review {
            let quarantine_id =
                format!("quarantine-{}-{}", job_started_at_ms, uuid::Uuid::new_v4());
            if let Err(err) = with_verified_memory_decrypt_principal(
                verified_tenant_context.as_deref(),
                quarantine_source_bound_import(
                    &manager,
                    &request.tenant_scope,
                    &binding.binding_id,
                    &source_objects,
                    job_started_at_ms,
                ),
            )
            .await
            {
                tracing::warn!(
                    error = %err,
                    "failed to quarantine enterprise source-bound import output"
                );
            }
            if let Err(err) = record_enterprise_ingestion_quarantine(
                &state,
                IngestionQuarantine {
                    quarantine_id: quarantine_id.clone(),
                    tenant_context: tenant_context.clone(),
                    connector_id: binding.connector_id.clone(),
                    binding_id: binding.binding_id.clone(),
                    source_object_ids: source_object_ids.clone(),
                    reason: "source binding requires ingestion review".to_string(),
                    created_at_ms: crate::util::time::now_ms(),
                    reviewed_by: None,
                    reviewed_at_ms: None,
                    disposition: None,
                },
            )
            .await
            {
                tracing::warn!(
                    error = %err,
                    "failed to record enterprise ingestion quarantine"
                );
            }
            Some(quarantine_id)
        } else {
            None
        };
        let job_state = if binding.require_review {
            IngestionJobState::Quarantined
        } else if stats.errors > 0 {
            IngestionJobState::Failed
        } else {
            IngestionJobState::Completed
        };
        if let Err(err) = record_enterprise_ingestion_job(
            &state,
            IngestionJob {
                job_id: job_id.clone(),
                tenant_context: tenant_context.clone(),
                connector_id: binding.connector_id.clone(),
                binding_id: binding.binding_id.clone(),
                state: job_state,
                source_object_ids,
                started_at_ms: Some(job_started_at_ms),
                finished_at_ms: Some(crate::util::time::now_ms()),
                quarantine_id,
            },
        )
        .await
        {
            tracing::warn!(
                error = %err,
                "failed to record enterprise ingestion job completion"
            );
        }
    }

    publish_tenant_event(
        &state,
        &tenant_context,
        "memory.import.succeeded",
        json!({
            "source": {"kind": "path", "path": path},
            "format": input.format,
            "tier": input.tier,
            "project_id": project_id.clone(),
            "session_id": session_id.clone(),
            "source_binding_id": source_binding_id.clone(),
            "sync_deletes": input.sync_deletes,
            "stats": {
                "discovered_files": stats.discovered_files,
                "files_processed": stats.files_processed,
                "indexed_files": stats.indexed_files,
                "skipped_files": stats.skipped_files,
                "deleted_files": stats.deleted_files,
                "chunks_created": stats.chunks_created,
                "errors": stats.errors,
            },
        }),
    );

    Ok(Json(memory_import_response(
        path,
        input.format,
        input.tier,
        project_id,
        session_id,
        source_binding_id,
        input.sync_deletes,
        stats,
    )))
}

async fn resolve_memory_import_source_binding(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    source_binding_id: Option<&str>,
) -> Result<Option<MemoryImportSourceBinding>, (StatusCode, Json<ErrorEnvelope>)> {
    let Some(source_binding_id) = source_binding_id else {
        if memory_import_requires_source_binding(
            tenant_context,
            request_principal,
            verified_tenant_context,
        ) {
            return Err(skill_error(
                StatusCode::BAD_REQUEST,
                "hosted/enterprise memory imports require source_binding_id",
            ));
        }
        return Ok(None);
    };
    if source_binding_id == DEFAULT_LOCAL_MANUAL_SOURCE_BINDING_ID
        && !memory_import_requires_source_binding(
            tenant_context,
            request_principal,
            verified_tenant_context,
        )
    {
        return Ok(Some(default_local_manual_source_binding(tenant_context)));
    }
    let registry = state.enterprise.source_bindings.read().await;
    let Some(binding) = registry.values().find(|binding| {
        binding.binding_id == source_binding_id && binding.tenant_matches(tenant_context)
    }) else {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source_binding_id does not reference an enabled binding for this tenant",
        ));
    };
    if !binding.state.allows_ingestion() || !binding.ingestion_policy.allow_indexing {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source binding does not allow memory import indexing",
        ));
    }
    let registry = state.enterprise.connectors.read().await;
    let Some(connector) = registry.values().find(|connector| {
        connector.connector_id == binding.connector_id && connector.tenant_matches(tenant_context)
    }) else {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "source binding connector is not registered for this tenant",
        ));
    };
    if !connector.state.allows_ingestion() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            format!(
                "source binding connector does not allow memory import indexing: {}",
                connector_lifecycle_state_label(connector.state)
            ),
        ));
    }
    // EAA-14 (TAN-39): apply the same fail-closed ingestion admission as the
    // connector import path so a manual `/memory/import` cannot bypass the
    // admin-label requirement or high-risk-data-class review. Manual imports
    // have no admin acknowledgement path, so review is never pre-acknowledged.
    let admission = tandem_enterprise_contract::evaluate_ingestion_admission(
        binding,
        connector,
        tandem_enterprise_contract::provider_acl_sync_mode(&connector.provider),
        false,
    );
    if let Some(reason) = admission.denied() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            format!("source binding ingestion denied: {}", reason.as_str()),
        ));
    }
    let require_review = admission.requires_review();
    Ok(Some(MemoryImportSourceBinding {
        binding_id: binding.binding_id.clone(),
        connector_id: binding.connector_id.clone(),
        resource_ref: serde_json::to_value(&binding.resource_ref).map_err(|_| {
            skill_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to serialize source binding resource scope",
            )
        })?,
        data_class: serde_json::to_value(binding.data_class)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| format!("{:?}", binding.data_class)),
        require_review,
    }))
}

const DEFAULT_LOCAL_MANUAL_SOURCE_BINDING_ID: &str = "local_manual_upload";

fn default_local_manual_source_binding(
    tenant_context: &TenantContext,
) -> MemoryImportSourceBinding {
    MemoryImportSourceBinding {
        binding_id: DEFAULT_LOCAL_MANUAL_SOURCE_BINDING_ID.to_string(),
        connector_id: "manual_upload".to_string(),
        resource_ref: json!({
            "organization_id": tenant_context.org_id.clone(),
            "workspace_id": tenant_context.workspace_id.clone(),
            "resource_kind": "document_collection",
            "resource_id": "local-manual-uploads",
        }),
        data_class: "internal".to_string(),
        require_review: false,
    }
}

async fn record_enterprise_ingestion_job(
    state: &AppState,
    job: IngestionJob,
) -> Result<(), std::io::Error> {
    let mut registry = state.enterprise.ingestion_jobs.write().await;
    let key = enterprise_ingestion_job_key(&job);
    registry.insert(key, job);
    persist_enterprise_ingestion_jobs(&state.enterprise.ingestion_jobs_path, &registry).await
}

async fn record_enterprise_ingestion_quarantine(
    state: &AppState,
    quarantine: IngestionQuarantine,
) -> Result<(), std::io::Error> {
    let mut registry = state.enterprise.ingestion_quarantines.write().await;
    let key = enterprise_ingestion_quarantine_key(&quarantine);
    registry.insert(key, quarantine);
    persist_enterprise_ingestion_quarantines(
        &state.enterprise.ingestion_quarantines_path,
        &registry,
    )
    .await
}

fn enterprise_ingestion_job_key(job: &IngestionJob) -> String {
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

fn enterprise_ingestion_quarantine_key(quarantine: &IngestionQuarantine) -> String {
    let deployment = quarantine
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        quarantine.tenant_context.org_id,
        quarantine.tenant_context.workspace_id,
        deployment,
        quarantine.quarantine_id
    )
}

async fn persist_enterprise_ingestion_jobs(
    path: &std::path::Path,
    registry: &std::collections::HashMap<String, IngestionJob>,
) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let payload = serde_json::to_vec_pretty(registry).map_err(std::io::Error::other)?;
    tokio::fs::write(path, payload).await
}

async fn persist_enterprise_ingestion_quarantines(
    path: &std::path::Path,
    registry: &std::collections::HashMap<String, IngestionQuarantine>,
) -> Result<(), std::io::Error> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let payload = serde_json::to_vec_pretty(registry).map_err(std::io::Error::other)?;
    tokio::fs::write(path, payload).await
}

async fn source_objects_seen_since(
    manager: &tandem_memory::MemoryManager,
    tenant_scope: &MemoryTenantScope,
    binding_id: &str,
    started_at_ms: u64,
) -> Result<Vec<SourceObjectLifecycleRecord>, tandem_memory::types::MemoryError> {
    let records = manager
        .store()
        .query(
            tandem_memory::MemoryStoreQueryRequest::SourceObjectLifecyclesForBinding {
                scope: tandem_memory::MemoryReadScope::tenant(tenant_scope.clone()),
                source_binding_id: binding_id.to_string(),
            },
        )
        .await
        .map_err(tandem_memory::types::MemoryError::from)?;
    let tandem_memory::MemoryStoreQueryResult::SourceObjectLifecycles(mut records) = records else {
        return Err(tandem_memory::types::MemoryError::InvalidConfig(
            "memory store returned an unexpected source lifecycle result".to_string(),
        ));
    };
    records.retain(|record| record.last_seen_at_ms >= started_at_ms);
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
            .store()
            .mutate(
                tandem_memory::MemoryStoreMutationRequest::DeleteChunksBySourcePath {
                    scope: tandem_memory::MemoryReadScope::tenant(tenant_scope.clone()),
                    selector: tandem_memory::MemoryChunkSelector {
                        tier: record.tier,
                        project_id: record.project_id.clone(),
                        session_id: record.session_id.clone(),
                    },
                    source_path: record.indexed_path.clone(),
                },
            )
            .await
            .map_err(tandem_memory::types::MemoryError::from)?;
        manager
            .store()
            .mutate(
                tandem_memory::MemoryStoreMutationRequest::DeleteImportIndexEntry {
                    scope: tandem_memory::MemoryReadScope::tenant(tenant_scope.clone()),
                    selector: tandem_memory::MemoryChunkSelector {
                        tier: record.tier,
                        project_id: record.project_id.clone(),
                        session_id: record.session_id.clone(),
                    },
                    path: record.indexed_path.clone(),
                },
            )
            .await
            .map_err(tandem_memory::types::MemoryError::from)?;
        manager
            .store()
            .mutate(
                tandem_memory::MemoryStoreMutationRequest::SetSourceObjectLifecycleState {
                    scope: tandem_memory::MemoryReadScope::tenant(tenant_scope.clone()),
                    source_binding_id: binding_id.to_string(),
                    source_object_id: record.source_object_id.clone(),
                    state: SourceObjectLifecycleState::Quarantined,
                    changed_at_ms,
                },
            )
            .await
            .map_err(tandem_memory::types::MemoryError::from)?;
    }
    Ok(())
}

include!("part02_import_helpers.rs");
