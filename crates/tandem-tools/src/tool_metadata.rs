use serde_json::Value;
use tandem_types::{
    AccessPermission, DataClass, ResourceKind, ToolCapabilities, ToolDomain, ToolEffect,
    ToolSchema, ToolSecurityDescriptor,
};

pub(crate) fn tool_schema(
    name: &'static str,
    description: impl Into<String>,
    input_schema: Value,
) -> ToolSchema {
    ToolSchema::new(name, description, input_schema)
}

pub(crate) fn tool_schema_with_capabilities(
    name: &'static str,
    description: impl Into<String>,
    input_schema: Value,
    capabilities: ToolCapabilities,
) -> ToolSchema {
    let security = security_descriptor_for_capabilities(&capabilities);
    ToolSchema::new(name, description, input_schema)
        .with_capabilities(capabilities)
        .with_security(security)
}

pub(crate) fn workspace_read_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Read)
        .domain(ToolDomain::Workspace)
        .reads_workspace()
        .preferred_for_discovery()
}

pub(crate) fn workspace_write_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Write)
        .domain(ToolDomain::Workspace)
        .writes_workspace()
        .requires_verification()
}

pub(crate) fn workspace_search_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Search)
        .domain(ToolDomain::Workspace)
        .reads_workspace()
        .preferred_for_discovery()
}

pub(crate) fn shell_execution_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Execute)
        .domain(ToolDomain::Shell)
        .reads_workspace()
        .writes_workspace()
        .network_access()
        .destructive()
        .requires_verification()
}

pub(crate) fn web_fetch_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Fetch)
        .domain(ToolDomain::Web)
        .network_access()
        .preferred_for_discovery()
}

pub(crate) fn apply_patch_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Patch)
        .domain(ToolDomain::Workspace)
        .reads_workspace()
        .writes_workspace()
        .requires_verification()
}

pub(crate) fn memory_search_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Search)
        .domain(ToolDomain::Memory)
        .preferred_for_discovery()
}

pub(crate) fn memory_read_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Read)
        .domain(ToolDomain::Memory)
        .preferred_for_discovery()
}

pub(crate) fn memory_write_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Write)
        .domain(ToolDomain::Memory)
        .requires_verification()
}

pub(crate) fn memory_delete_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Delete)
        .domain(ToolDomain::Memory)
        .destructive()
        .requires_verification()
}

pub(crate) fn planning_write_capabilities() -> ToolCapabilities {
    ToolCapabilities::new()
        .effect(ToolEffect::Write)
        .domain(ToolDomain::Planning)
}

fn security_descriptor_for_capabilities(capabilities: &ToolCapabilities) -> ToolSecurityDescriptor {
    let mut security = ToolSecurityDescriptor::new();

    if capabilities.reads_workspace {
        security = security
            .permission(AccessPermission::Read)
            .resource_kind(ResourceKind::Directory)
            .resource_kind(ResourceKind::File)
            .resource_kind(ResourceKind::Repository)
            .data_class(DataClass::Internal)
            .data_class(DataClass::SourceCode);
    }

    if capabilities.writes_workspace {
        security = security
            .permission(AccessPermission::Edit)
            .resource_kind(ResourceKind::Artifact);
    }

    if capabilities.domains.contains(&ToolDomain::Shell)
        || capabilities.effects.contains(&ToolEffect::Execute)
    {
        security = security
            .permission(AccessPermission::Execute)
            .external_side_effect();
    }

    if capabilities.network_access
        && (capabilities.effects.contains(&ToolEffect::Write)
            || capabilities.effects.contains(&ToolEffect::Patch)
            || capabilities.effects.contains(&ToolEffect::Delete)
            || capabilities.effects.contains(&ToolEffect::Execute))
    {
        security = security.external_side_effect();
    }

    if capabilities.domains.contains(&ToolDomain::Web) {
        security = security
            .permission(AccessPermission::View)
            .data_class(DataClass::Public);
    }

    if capabilities.domains.contains(&ToolDomain::Memory) {
        let mutates_memory = capabilities.effects.contains(&ToolEffect::Write)
            || capabilities.effects.contains(&ToolEffect::Patch)
            || capabilities.effects.contains(&ToolEffect::Delete);
        let permission = if mutates_memory {
            AccessPermission::Edit
        } else {
            AccessPermission::Read
        };
        security = security
            .permission(permission)
            .resource_kind(ResourceKind::MemorySpace)
            .resource_kind(ResourceKind::KnowledgeSpace)
            .data_class(DataClass::Internal)
            .data_class(DataClass::Confidential);
    }

    // Planning-domain tools (e.g. `todo_write`) are local, non-resource helpers.
    // They intentionally get no security descriptor so they remain visible in
    // read-scoped strict contexts, matching their pre-annotation behavior; the
    // capability metadata still records the planning effect/domain.

    security
}
