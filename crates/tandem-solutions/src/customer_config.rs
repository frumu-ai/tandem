// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::validate::{digest, identifier};
use crate::*;
use std::collections::{BTreeMap, BTreeSet};
use tandem_enterprise_contract::VerifiedTenantContext;

/// All approved sets and the selected installation come from the trusted host
/// for this caller. Never deserialize these inputs from the customer document.
pub struct CustomerConfigInput<'a> {
    pub verified_context: &'a VerifiedTenantContext,
    pub selected_scope: &'a CustomerScope,
    pub now_ms: u64,
    pub current_revision: Option<&'a str>,
    pub expected_revision: Option<&'a str>,
    pub host_policy: &'a Constraints,
    pub approved_references: &'a BTreeSet<String>,
    pub approved_connectors: &'a BTreeMap<String, ConnectorBinding>,
    pub approved_subjects: &'a BTreeSet<String>,
    pub approved_org_units: &'a BTreeSet<String>,
    pub approved_projects: &'a BTreeSet<String>,
}

fn invalid(path: &str) -> SolutionError {
    SolutionError::new(
        "invalid_customer_config",
        path,
        "Invalid customer configuration field",
    )
}

fn reference(value: &str, prefix: &str, path: &str) -> Result<(), SolutionError> {
    let suffix = value.strip_prefix(prefix).ok_or_else(|| invalid(path))?;
    identifier(suffix, path)
}

pub fn parse_customer_config(input: &str) -> Result<CustomerConfig, SolutionError> {
    if input.len() > MAX_BLUEPRINT_BYTES {
        return Err(SolutionError::new(
            "size_limit",
            "$",
            "Customer configuration exceeds 1 MiB",
        ));
    }
    // As with blueprints, reject duplicate YAML/JSON keys before typed maps.
    let value: serde_yaml::Value = serde_yaml::from_str(input).map_err(|_| invalid("$"))?;
    let config: CustomerConfig = serde_yaml::from_value(value).map_err(|_| invalid("$"))?;
    validate_customer_config(&config)?;
    Ok(config)
}

pub fn validate_customer_config(config: &CustomerConfig) -> Result<(), SolutionError> {
    if config.schema_version != SCHEMA_VERSION {
        return Err(SolutionError::new(
            "unsupported_schema",
            "schema_version",
            "Expected string version 1",
        ));
    }
    for (path, value) in [
        ("scope.org_id", &config.scope.org_id),
        ("scope.workspace_id", &config.scope.workspace_id),
        ("scope.deployment_id", &config.scope.deployment_id),
        ("scope.instance_id", &config.scope.instance_id),
    ] {
        identifier(value, path)?;
    }
    reference(&config.profile_ref, "profile-ref:", "profile_ref")?;
    if config.timezone.is_empty()
        || config.timezone.len() > 80
        || config.timezone.contains("..")
        || !config
            .timezone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
    {
        return Err(invalid("timezone"));
    }
    if config.locale.is_empty()
        || config.locale.len() > 35
        || !config
            .locale
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(invalid("locale"));
    }
    if [
        config.optional_components.len(),
        config.preferences.len(),
        config.connectors.len(),
        config.models.len(),
        config.secret_refs.len(),
        config.data_refs.len(),
        config.memory_spaces.len(),
    ]
    .into_iter()
    .any(|length| length > MAX_COMPONENTS)
    {
        return Err(SolutionError::new(
            "size_limit",
            "$",
            "Customer collections exceed 256 entries",
        ));
    }
    for (path, references, prefix) in [
        ("secret_refs", &config.secret_refs, "secret-ref:"),
        ("data_refs", &config.data_refs, "data-ref:"),
    ] {
        for (slot, value) in references {
            identifier(slot, path)?;
            reference(value, prefix, path)?;
        }
    }
    for (slot, space) in &config.memory_spaces {
        identifier(slot, "memory_spaces")?;
        match space {
            CustomerMemorySpace::PrivateUser { subject_id } => {
                identifier(subject_id, "memory_spaces.subject_id")?
            }
            CustomerMemorySpace::DepartmentShared { org_unit_id } => {
                identifier(org_unit_id, "memory_spaces.org_unit_id")?
            }
            CustomerMemorySpace::Project { project_id } => {
                identifier(project_id, "memory_spaces.project_id")?
            }
            CustomerMemorySpace::TenantShared => (),
        }
    }
    Ok(())
}

pub fn customer_config_revision(config: &CustomerConfig) -> Result<String, SolutionError> {
    validate_customer_config(config)?;
    Ok(sha256(&canonical_json(config)?))
}

/// Validate the caller's current verified scope without reading customer data.
/// The runtime must separately authorize the explicitly selected installation.
pub fn validate_customer_config_scope(
    context: &VerifiedTenantContext,
    scope: &CustomerScope,
    now_ms: u64,
) -> Result<AuthorityBinding, SolutionError> {
    let authority = crate::resolve::authority_binding(context, now_ms)?;
    identifier(&scope.instance_id, "scope.instance_id")?;
    if scope.org_id != authority.org_id
        || scope.workspace_id != authority.workspace_id
        || scope.deployment_id != authority.deployment_id
    {
        return Err(SolutionError::new(
            "customer_scope_mismatch",
            "scope",
            "Select the authorized organization and installation",
        ));
    }
    Ok(authority)
}

/// Produces inputs for the existing resolver, never an activation receipt.
/// The eventual apply transaction must repeat the revision/authority checks
/// atomically with persistence; this pure check does not lock a database.
pub fn prepare_customer_config(
    blueprint: &SolutionBlueprint,
    config: &CustomerConfig,
    input: CustomerConfigInput<'_>,
) -> Result<PreparedCustomerConfig, SolutionError> {
    validate_blueprint(blueprint)?;
    let revision = customer_config_revision(config)?;
    validate_customer_config_scope(input.verified_context, input.selected_scope, input.now_ms)?;
    if config.scope != *input.selected_scope {
        return Err(SolutionError::new(
            "customer_scope_mismatch",
            "scope",
            "Select the authorized organization and installation",
        ));
    }
    for value in [input.current_revision, input.expected_revision]
        .into_iter()
        .flatten()
    {
        digest(value, "expected_revision")?;
    }
    if input.current_revision != input.expected_revision {
        return Err(SolutionError::new(
            "customer_revision_conflict",
            "expected_revision",
            "Configuration changed; preview the current revision",
        ));
    }
    for value in std::iter::once(&config.profile_ref)
        .chain(config.secret_refs.values())
        .chain(config.data_refs.values())
    {
        if !input.approved_references.contains(value) {
            return Err(SolutionError::new(
                "customer_reference_denied",
                "references",
                "Host must approve each reference for this scope and caller",
            ));
        }
    }
    for (capability, binding) in &config.connectors {
        if input.approved_connectors.get(capability) != Some(binding) {
            return Err(SolutionError::new(
                "customer_connector_denied",
                "connectors",
                "CapabilityResolver must approve the current account generation",
            ));
        }
    }
    if !config
        .memory_spaces
        .keys()
        .eq(blueprint.memory_spaces.keys())
    {
        return Err(SolutionError::new(
            "customer_memory_binding_denied",
            "memory_spaces",
            "Bind every declared memory space exactly once",
        ));
    }
    for (slot, space) in &config.memory_spaces {
        let (kind, approved) = match space {
            CustomerMemorySpace::PrivateUser { subject_id } => (
                MemorySpace::PrivateUser,
                input.approved_subjects.contains(subject_id),
            ),
            CustomerMemorySpace::DepartmentShared { org_unit_id } => (
                MemorySpace::DepartmentShared,
                input.approved_org_units.contains(org_unit_id),
            ),
            CustomerMemorySpace::Project { project_id } => (
                MemorySpace::Project,
                input.approved_projects.contains(project_id),
            ),
            CustomerMemorySpace::TenantShared => (MemorySpace::TenantShared, true),
        };
        if blueprint.memory_spaces.get(slot) != Some(&kind) || !approved {
            return Err(SolutionError::new(
                "customer_memory_binding_denied",
                "memory_spaces",
                "Bind a declared space to an existing authorized subject, department or project",
            ));
        }
    }
    Ok(PreparedCustomerConfig {
        request: InstallRequest {
            instance_id: config.scope.instance_id.clone(),
            customer_config_revision: revision,
            optional_components: config.optional_components.clone(),
            preferences: config.preferences.clone(),
            connectors: config.connectors.clone(),
            models: config.models.clone(),
        },
        deployment_policy: crate::resolve::intersect_constraints(
            &config.constraints,
            input.host_policy,
        ),
    })
}

/// A shareable skeleton, not a deployable export. Derive it from the blueprint
/// alone so even customer identifiers hidden in preference/reference keys or
/// values cannot escape. Customer-owned backup uses CustomerConfig separately.
pub fn customer_config_template(
    blueprint: &SolutionBlueprint,
) -> Result<CustomerConfigTemplate, SolutionError> {
    validate_blueprint(blueprint)?;
    Ok(CustomerConfigTemplate {
        schema_version: SCHEMA_VERSION.into(),
        template_kind: "customer-config-template".into(),
        solution: blueprint.solution.clone(),
        optional_components: blueprint
            .components
            .iter()
            .filter(|(_, c)| !c.required)
            .map(|(id, _)| id.clone())
            .collect(),
        model_slots: blueprint
            .components
            .values()
            .flat_map(|c| c.model_classes.iter().cloned())
            .collect(),
        connector_slots: blueprint
            .components
            .values()
            .flat_map(|c| {
                c.required_capabilities
                    .iter()
                    .chain(&c.optional_capabilities)
                    .cloned()
            })
            .collect(),
        requires_customer_configuration: true,
    })
}

pub fn customer_config_changes(
    previous_blueprint: &SolutionBlueprint,
    next_blueprint: &SolutionBlueprint,
    previous: &CustomerConfig,
    next: &CustomerConfig,
) -> Result<CustomerConfigChanges, SolutionError> {
    validate_customer_config(previous)?;
    validate_customer_config(next)?;
    let groups = [
        ("scope", previous.scope != next.scope),
        ("profile_ref", previous.profile_ref != next.profile_ref),
        ("timezone", previous.timezone != next.timezone),
        ("locale", previous.locale != next.locale),
        (
            "optional_components",
            previous.optional_components != next.optional_components,
        ),
        ("preferences", previous.preferences != next.preferences),
        ("connectors", previous.connectors != next.connectors),
        ("models", previous.models != next.models),
        ("secret_refs", previous.secret_refs != next.secret_refs),
        ("data_refs", previous.data_refs != next.data_refs),
        (
            "memory_spaces",
            previous.memory_spaces != next.memory_spaces,
        ),
        ("constraints", previous.constraints != next.constraints),
    ];
    Ok(CustomerConfigChanges {
        upstream_changed: blueprint_hash(previous_blueprint)? != blueprint_hash(next_blueprint)?,
        customer_fields: groups
            .into_iter()
            .filter(|(_, changed)| *changed)
            .map(|(name, _)| name.to_string())
            .collect(),
    })
}
