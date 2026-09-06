// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::validate::*;
use crate::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use tandem_enterprise_contract::{TenantSource, VerifiedTenantContext};

/// Supplied by the trusted host after existing assertion verification and
/// CapabilityResolver readiness checks. Never deserialize this from a browser,
/// model response, or solution pack. No network or wall clock access occurs here.
pub struct ResolutionInput<'a> {
    /// Digest of the host-owned source and provider routing snapshot. Callers
    /// that install resources must supply it; None preserves pure legacy plans.
    pub host_facts_sha256: Option<&'a str>,
    pub request: &'a InstallRequest,
    pub verified_context: &'a VerifiedTenantContext,
    pub now_ms: u64,
    pub engine_version: &'a str,
    pub deployment_policy: &'a Constraints,
    pub available_deployment_requirements: &'a BTreeSet<String>,
    /// Binding ID -> current model/account metadata, approved for the verified
    /// caller by the host. Never deserialize this registry from InstallRequest.
    pub approved_models: &'a BTreeMap<String, ModelBinding>,
    /// Exact bytes obtained by PackManager, keyed by component ID.
    pub artifacts: &'a BTreeMap<String, Vec<u8>>,
}

pub fn resolve(
    blueprint: &SolutionBlueprint,
    input: ResolutionInput<'_>,
) -> Result<ResolvedPlan, SolutionError> {
    validate_blueprint(blueprint)?;
    if let Some(value) = input.host_facts_sha256 {
        digest(value, "host_facts_sha256")?;
    }
    let request = input.request;
    identifier(&request.instance_id, "instance_id")?;
    digest(
        &request.customer_config_revision,
        "customer_config_revision",
    )?;
    let engine = version(input.engine_version, "engine_version")?;
    if !requirement(&blueprint.engine_version, "engine_version")?.matches(&engine) {
        return Err(SolutionError::new(
            "incompatible_engine",
            "engine_version",
            "Engine does not satisfy this blueprint's requirement",
        ));
    }
    let authority = authority_binding(input.verified_context, input.now_ms)?;
    for needed in &blueprint.deployment_requirements {
        if !input.available_deployment_requirements.contains(needed) {
            return Err(SolutionError::new(
                "deployment_requirement_missing",
                format!("deployment_requirements.{needed}"),
                "Host readiness check must satisfy this requirement",
            ));
        }
    }
    let constraints = intersect_constraints(&blueprint.constraints, input.deployment_policy);
    let mut selected: BTreeSet<String> = blueprint
        .components
        .iter()
        .filter(|(_, c)| c.required)
        .map(|(id, _)| id.clone())
        .collect();
    for id in &request.optional_components {
        let component = blueprint.components.get(id).ok_or_else(|| {
            SolutionError::new(
                "unknown_component",
                format!("optional_components.{id}"),
                "Optional component is not declared",
            )
        })?;
        if component.required {
            return Err(SolutionError::new(
                "not_optional",
                format!("optional_components.{id}"),
                "Required components are always selected",
            ));
        }
        selected.insert(id.clone());
    }
    // Optional selection pulls in its transitive prerequisites, never unrelated
    // optional modules. Validation above bounds this graph and rejects cycles.
    loop {
        let before = selected.len();
        let dependencies: Vec<_> = selected
            .iter()
            .flat_map(|id| blueprint.components[id].depends_on.keys().cloned())
            .collect();
        selected.extend(dependencies);
        if selected.len() == before {
            break;
        }
    }
    let install_order = topological_order(&blueprint.components, &selected)?;
    if install_order.is_empty() {
        return Err(SolutionError::new(
            "empty_selection",
            "components",
            "Select at least one component",
        ));
    }
    let mut components = BTreeMap::new();
    let mut required_capabilities = BTreeSet::new();
    let mut optional_capabilities = BTreeSet::new();
    let mut model_classes = BTreeSet::new();
    // Tenant-qualified namespace prevents the same logical instance name in a
    // different workspace from claiming existing resources.
    let namespace = sha256(&canonical_json(&json!({
        "org": authority.org_id, "workspace": authority.workspace_id,
        "deployment": authority.deployment_id, "instance": request.instance_id
    }))?);
    for id in &selected {
        let component = &blueprint.components[id];
        if let Some(conflict) = component.conflicts_with.intersection(&selected).next() {
            return Err(SolutionError::new(
                "component_conflict",
                format!("components.{id}.conflicts_with.{conflict}"),
                "Conflicting components cannot be installed together",
            ));
        }
        let bytes = input.artifacts.get(id).ok_or_else(|| {
            SolutionError::new(
                "artifact_missing",
                format!("components.{id}.artifact"),
                "PackManager must supply the selected entry bytes",
            )
        })?;
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(SolutionError::new(
                "size_limit",
                format!("components.{id}.artifact"),
                "Artifact exceeds 8 MiB",
            ));
        }
        if sha256(bytes) != component.artifact.sha256 {
            return Err(SolutionError::new(
                "artifact_digest_mismatch",
                format!("components.{id}.artifact.sha256"),
                "Entry bytes differ from the pinned digest",
            ));
        }
        components.insert(
            id.clone(),
            LockedComponent {
                resource_id: format!("solution-{namespace}-{id}"),
                owner_instance_id: request.instance_id.clone(),
                kind: component.kind.clone(),
                artifact: component.artifact.clone(),
                depends_on: component.depends_on.keys().cloned().collect(),
            },
        );
        required_capabilities.extend(component.required_capabilities.iter().cloned());
        optional_capabilities.extend(component.optional_capabilities.iter().cloned());
        model_classes.extend(component.model_classes.iter().cloned());
    }
    optional_capabilities = optional_capabilities
        .difference(&required_capabilities)
        .cloned()
        .collect();
    for required in &required_capabilities {
        if !request.connectors.contains_key(required) {
            return Err(SolutionError::new(
                "capability_unbound",
                format!("connectors.{required}"),
                "Bind this required capability through CapabilityResolver",
            ));
        }
    }
    for (capability, binding) in &request.connectors {
        if !required_capabilities.contains(capability)
            && !optional_capabilities.contains(capability)
        {
            return Err(SolutionError::new(
                "undeclared_binding",
                format!("connectors.{capability}"),
                "Selected components do not request this capability",
            ));
        }
        nonsecret_reference(
            &binding.connection_id,
            &format!("connectors.{capability}.connection_id"),
        )?;
        nonsecret_reference(
            &binding.generation,
            &format!("connectors.{capability}.generation"),
        )?;
    }
    for class in &model_classes {
        if !request.models.contains_key(class) {
            return Err(SolutionError::new(
                "model_unbound",
                format!("models.{class}"),
                "Bind this model class through the host model profile",
            ));
        }
    }
    let mut models = BTreeMap::new();
    for (class, binding_id) in &request.models {
        if !model_classes.contains(class) {
            return Err(SolutionError::new(
                "undeclared_binding",
                format!("models.{class}"),
                "Selected components do not request this model class",
            ));
        }
        nonsecret_reference(binding_id, &format!("models.{class}"))?;
        let binding = input.approved_models.get(binding_id).ok_or_else(|| {
            SolutionError::new(
                "model_binding_unapproved",
                format!("models.{class}"),
                "Select a current model binding approved by the host for this caller",
            )
        })?;
        if !constraints.allowed_providers.contains(&binding.provider)
            || (binding.uses_network && !constraints.allow_network_egress)
        {
            return Err(SolutionError::new(
                "provider_denied",
                format!("models.{class}"),
                "Binding exceeds the effective provider or network policy",
            ));
        }
        nonsecret_reference(&binding.model, &format!("models.{class}.model"))?;
        nonsecret_reference(
            &binding.credential_ref,
            &format!("models.{class}.credential_ref"),
        )?;
        models.insert(
            class.clone(),
            LockedModelBinding {
                binding_id: binding_id.clone(),
                binding: binding.clone(),
            },
        );
    }
    let mut preferences: BTreeMap<_, _> = blueprint
        .preferences
        .iter()
        .map(|(name, preference)| (name.clone(), default_preference(preference)))
        .collect();
    for (name, value) in &request.preferences {
        let definition = blueprint.preferences.get(name).ok_or_else(|| {
            SolutionError::new(
                "unknown_preference",
                format!("preferences.{name}"),
                "Preference is not declared; policy fields cannot be overridden",
            )
        })?;
        check_preference(name, definition, value, true)?;
        preferences.insert(name.clone(), value.clone());
    }
    let unresolved_optional_capabilities = optional_capabilities
        .iter()
        .filter(|cap| !request.connectors.contains_key(*cap))
        .cloned()
        .collect();
    Ok(ResolvedPlan {
        host_facts_sha256: input.host_facts_sha256.map(str::to_owned),
        schema_version: SCHEMA_VERSION.into(),
        resolver_version: RESOLVER_VERSION.into(),
        engine_version: engine.to_string(),
        blueprint_sha256: blueprint_hash(blueprint)?,
        solution: blueprint.solution.clone(),
        instance_id: request.instance_id.clone(),
        customer_config_revision: request.customer_config_revision.clone(),
        authority,
        components,
        install_order,
        required_capabilities,
        optional_capabilities,
        unresolved_optional_capabilities,
        connectors: request.connectors.clone(),
        models,
        preferences,
        memory_spaces: blueprint.memory_spaces.clone(),
        constraints,
        deployment_requirements: blueprint.deployment_requirements.clone(),
        ui_features: blueprint.ui_features.clone(),
    })
}

pub(crate) fn intersect_constraints(blueprint: &Constraints, policy: &Constraints) -> Constraints {
    Constraints {
        allowed_providers: blueprint
            .allowed_providers
            .intersection(&policy.allowed_providers)
            .cloned()
            .collect(),
        allow_network_egress: blueprint.allow_network_egress && policy.allow_network_egress,
        max_tokens_per_run: blueprint.max_tokens_per_run.min(policy.max_tokens_per_run),
        max_concurrent_runs: blueprint
            .max_concurrent_runs
            .min(policy.max_concurrent_runs),
        max_daily_cost_microusd: blueprint
            .max_daily_cost_microusd
            .min(policy.max_daily_cost_microusd),
    }
}

fn nonsecret_reference(value: &str, path: &str) -> Result<(), SolutionError> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/:".contains(&b))
    {
        return Err(SolutionError::new(
            "invalid_reference",
            path,
            "Expected an opaque ID/reference, not credentials or configuration data",
        ));
    }
    Ok(())
}

pub(crate) fn authority_binding(
    context: &VerifiedTenantContext,
    now_ms: u64,
) -> Result<AuthorityBinding, SolutionError> {
    let tenant = &context.tenant_context;
    let actor = &context.human_actor.actor_id;
    let deployment = tenant.deployment_id.as_deref().unwrap_or("");
    if tenant.source != TenantSource::Explicit
        || tenant.org_id.trim().is_empty()
        || tenant.workspace_id.trim().is_empty()
        || deployment.trim().is_empty()
        || actor.trim().is_empty()
        || tenant.actor_id.as_deref() != Some(actor.as_str())
        || context.authority_chain.initiated_by.actor_id.as_deref() != Some(actor.as_str())
        || context.is_expired_at(now_ms)
        || context.issued_at_ms > now_ms
    {
        return Err(SolutionError::new("verified_identity_required", "authority", "Provide a current, verified user and explicit tenant/workspace/deployment; local single-human mode is unsupported"));
    }
    // Exclude rotating assertion IDs/timestamps/keys from deployment identity.
    // Include the complete authorization projection, removing only its assertion
    // envelope. Ordering of its arrays is retained conservatively.
    let mut strict = serde_json::to_value(&context.strict_projection)
        .map_err(|_| SolutionError::new("serialization", "authority", "Cannot encode authority"))?;
    if let Some(object) = strict.as_object_mut() {
        object.remove("assertion");
    }
    let set = |values: &Vec<String>| values.iter().cloned().collect::<BTreeSet<_>>();
    let authority_sha256 = sha256(&canonical_json(&json!({
        "issuer": context.issuer, "audience": context.audience,
        "roles": set(&context.roles), "org_units": set(&context.org_units),
        "capabilities": set(&context.capabilities), "policy_version": context.policy_version,
        "authority_chain": context.authority_chain, "strict_projection": strict
    }))?);
    Ok(AuthorityBinding {
        org_id: tenant.org_id.clone(),
        workspace_id: tenant.workspace_id.clone(),
        deployment_id: deployment.into(),
        actor_id: actor.clone(),
        authority_sha256,
    })
}
