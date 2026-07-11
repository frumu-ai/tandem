fn default_memory_import_format() -> MemoryImportFormat {
    MemoryImportFormat::Directory
}

fn default_memory_import_tier() -> MemoryTier {
    MemoryTier::Project
}

fn tenant_context_event_value(tenant_context: &TenantContext) -> Value {
    serde_json::to_value(tenant_context).unwrap_or_else(|_| json!(tenant_context))
}

fn with_tenant_context(mut properties: Value, tenant_context: &TenantContext) -> Value {
    if let Some(map) = properties.as_object_mut() {
        map.insert(
            "tenantContext".to_string(),
            tenant_context_event_value(tenant_context),
        );
    }
    properties
}
