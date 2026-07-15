// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn connector_lifecycle_state_label(state: ConnectorLifecycleState) -> &'static str {
    match state {
        ConnectorLifecycleState::Active => "active",
        ConnectorLifecycleState::Paused => "paused",
        ConnectorLifecycleState::Revoked => "revoked",
        ConnectorLifecycleState::Quarantined => "quarantined",
    }
}

fn memory_import_requires_source_binding(
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> bool {
    verified_tenant_context.is_some()
        || tenant_context.deployment_id.is_some()
        || request_principal.source == "tandem-web"
}

fn memory_import_response(
    path: String,
    format: MemoryImportFormat,
    tier: MemoryTier,
    project_id: Option<String>,
    session_id: Option<String>,
    source_binding_id: Option<String>,
    sync_deletes: bool,
    stats: MemoryImportStats,
) -> MemoryImportResponse {
    MemoryImportResponse {
        ok: true,
        source: MemoryImportPathSourceResponse { kind: "path", path },
        format,
        tier,
        project_id,
        session_id,
        source_binding_id,
        sync_deletes,
        discovered_files: stats.discovered_files,
        files_processed: stats.files_processed,
        indexed_files: stats.indexed_files,
        skipped_files: stats.skipped_files,
        deleted_files: stats.deleted_files,
        chunks_created: stats.chunks_created,
        errors: stats.errors,
    }
}

fn normalize_optional_memory_import_id(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().to_string())
        .filter(|trimmed| !trimmed.is_empty())
}

fn validate_memory_import_path(path: &str) -> Result<(), (StatusCode, Json<ErrorEnvelope>)> {
    let metadata = std::fs::metadata(path).map_err(|err| {
        skill_error(
            StatusCode::BAD_REQUEST,
            format!("source.path must exist and be readable: {err}"),
        )
    })?;
    let readable = if metadata.is_dir() {
        std::fs::read_dir(path).map(|_| ())
    } else {
        std::fs::File::open(path).map(|_| ())
    };
    readable.map_err(|err| {
        skill_error(
            StatusCode::BAD_REQUEST,
            format!("source.path must be readable: {err}"),
        )
    })
}
