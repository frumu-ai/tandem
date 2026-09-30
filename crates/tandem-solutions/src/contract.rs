// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SolutionBlueprint {
    #[schemars(schema_with = "schema_version")]
    pub schema_version: String,
    pub solution: SolutionIdentity,
    /// Semver requirement, checked against the target engine, not this crate.
    pub engine_version: String,
    pub components: BTreeMap<String, Component>,
    #[serde(default)]
    pub memory_spaces: BTreeMap<String, MemorySpace>,
    #[serde(default)]
    pub preferences: BTreeMap<String, Preference>,
    pub constraints: Constraints,
    #[serde(default)]
    pub deployment_requirements: BTreeSet<String>,
    #[serde(default)]
    pub ui_features: BTreeSet<String>,
}

fn schema_version(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    schemars::schema::SchemaObject {
        instance_type: Some(schemars::schema::InstanceType::String.into()),
        enum_values: Some(vec![serde_json::Value::String(
            crate::SCHEMA_VERSION.into(),
        )]),
        ..Default::default()
    }
    .into()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SolutionIdentity {
    pub id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub kind: ComponentKind,
    pub required: bool,
    pub artifact: ArtifactRef,
    /// Component ID -> compatible exact artifact version requirement.
    #[serde(default)]
    pub depends_on: BTreeMap<String, String>,
    #[serde(default)]
    pub conflicts_with: BTreeSet<String>,
    #[serde(default)]
    pub required_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub optional_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub model_classes: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    AgentTemplate,
    AgentPreset,
    WorkerProfile,
    Routine,
    Workflow,
    Goal,
    Policy,
    Ontology,
    ConnectorRecipe,
    ModelProfile,
    Onboarding,
    Ui,
    Evaluation,
}

/// References existing pack contents. This is not a second archive format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub pack_id: String,
    pub version: String,
    /// Lowercase SHA-256 of the entry's exact bytes.
    pub sha256: String,
    /// Portable relative pack entry path, never a URL or a host path.
    pub path: String,
}

/// All v1 spaces use governed memory_records. They are labels over supported
/// columns, not new team/curated stores. Writers resolve subjects/partitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemorySpace {
    PrivateUser,
    DepartmentShared,
    TenantShared,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Preference {
    Boolean {
        default: bool,
        overridable: bool,
    },
    Integer {
        default: u64,
        min: u64,
        max: u64,
        overridable: bool,
    },
    Choice {
        default: String,
        choices: BTreeSet<String>,
        overridable: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PreferenceValue {
    Boolean(bool),
    Integer(u64),
    Choice(String),
}

/// Non-overridable ceilings. Deployment policy can only narrow these values.
/// No allow-all sentinel: an empty provider set permits no model providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Constraints {
    pub allowed_providers: BTreeSet<String>,
    pub allow_network_egress: bool,
    pub max_tokens_per_run: u64,
    pub max_concurrent_runs: u32,
    pub max_daily_cost_microusd: u64,
}

/// Configuration contains references and bounded preferences only. Names,
/// documents, credentials and private data belong to separate customer stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallRequest {
    pub instance_id: String,
    pub customer_config_revision: String,
    #[serde(default)]
    pub optional_components: BTreeSet<String>,
    #[serde(default)]
    pub preferences: BTreeMap<String, PreferenceValue>,
    #[serde(default)]
    pub connectors: BTreeMap<String, ConnectorBinding>,
    /// Model class -> opaque binding ID approved by the host for this caller.
    /// Provider, credential and network metadata cannot come from the request.
    #[serde(default)]
    pub models: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorBinding {
    /// CapabilityResolver's approved account ID and current generation.
    pub connection_id: String,
    pub generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Model metadata from the trusted host registry, never an install request.
pub struct ModelBinding {
    pub provider: String,
    pub model: String,
    pub credential_ref: String,
    pub uses_network: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedModelBinding {
    pub binding_id: String,
    pub binding: ModelBinding,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedComponent {
    pub resource_id: String,
    pub owner_instance_id: String,
    pub kind: ComponentKind,
    pub artifact: ArtifactRef,
    pub depends_on: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityBinding {
    pub org_id: String,
    pub workspace_id: String,
    pub deployment_id: String,
    pub actor_id: String,
    /// Stable authorization projection; no assertions, signatures or tokens.
    pub authority_sha256: String,
}

/// A review artifact, never a bearer capability. Apply must reauthorize and
/// recheck current identities, bindings, policies, digests and ownership.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedPlan {
    /// Current host facts are part of the reviewed composition, not authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_facts_sha256: Option<String>,
    pub schema_version: String,
    pub resolver_version: String,
    pub engine_version: String,
    pub blueprint_sha256: String,
    pub solution: SolutionIdentity,
    pub instance_id: String,
    pub customer_config_revision: String,
    pub authority: AuthorityBinding,
    pub components: BTreeMap<String, LockedComponent>,
    pub install_order: Vec<String>,
    pub required_capabilities: BTreeSet<String>,
    pub optional_capabilities: BTreeSet<String>,
    pub unresolved_optional_capabilities: BTreeSet<String>,
    pub connectors: BTreeMap<String, ConnectorBinding>,
    pub models: BTreeMap<String, LockedModelBinding>,
    pub preferences: BTreeMap<String, PreferenceValue>,
    pub memory_spaces: BTreeMap<String, MemorySpace>,
    pub constraints: Constraints,
    pub deployment_requirements: BTreeSet<String>,
    pub ui_features: BTreeSet<String>,
}

impl ResolvedPlan {
    /// Hash is computed over the entire lock, never embedded into its own input.
    pub fn composition_hash(&self) -> Result<String, crate::SolutionError> {
        Ok(crate::sha256(&crate::canonical_json(self)?))
    }
}
