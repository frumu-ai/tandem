// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Customer-owned document, never reusable pack content or an authority grant.
/// Human names, aliases and imported content stay in the referenced profile/data
/// stores. This document contains no credential values or connection settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerConfig {
    pub schema_version: String,
    /// Stable reusable solution identity; versions may advance independently.
    pub solution_id: String,
    pub scope: CustomerScope,
    pub profile_ref: String,
    pub timezone: String,
    pub locale: String,
    #[serde(default)]
    pub optional_components: BTreeSet<String>,
    #[serde(default)]
    pub preferences: BTreeMap<String, PreferenceValue>,
    #[serde(default)]
    pub connectors: BTreeMap<String, ConnectorBinding>,
    #[serde(default)]
    pub models: BTreeMap<String, String>,
    #[serde(default)]
    pub secret_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub data_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub memory_spaces: BTreeMap<String, CustomerMemorySpace>,
    pub constraints: Constraints,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerScope {
    pub org_id: String,
    pub workspace_id: String,
    pub deployment_id: String,
    pub instance_id: String,
}

/// Declarations bind existing authorized resources; they do not create users,
/// memberships, knowledge grants, or team/curated backing stores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CustomerMemorySpace {
    PrivateUser { subject_id: String },
    DepartmentShared { org_unit_id: String },
    TenantShared,
    Project { project_id: String },
}

/// Immutable, non-serializable preparation bound to its blueprint and authority.
/// Resolve through `PreparedCustomerConfig::resolve`; no raw request can escape.
/// ```compile_fail
/// fn unbind(prepared: tandem_solutions::PreparedCustomerConfig) {
///     let _ = prepared.request;
/// }
/// ```
/// ```compile_fail
/// fn serialize(prepared: tandem_solutions::PreparedCustomerConfig) {
///     let _ = serde_json::to_string(&prepared);
/// }
/// ```
pub struct PreparedCustomerConfig {
    pub(crate) request: InstallRequest,
    /// Customer-owned snapshot validated with this request's revision. Retain
    /// profile, locale, secret/data references and concrete memory bindings for
    /// runtime adapters without rereading a potentially changed document.
    /// This is not a reusable export or an authorization/activation receipt.
    pub(crate) customer_config: CustomerConfig,
    /// Stored ceilings intersect again with fresh host policy and the blueprint
    /// during resolution. Callers cannot replace this validated policy.
    pub(crate) deployment_policy: Constraints,
    pub(crate) blueprint_sha256: String,
    pub(crate) authority: AuthorityBinding,
}

/// Public template derives only from the reusable blueprint. No customer
/// values, IDs, reference names, hashes, bindings or override values are copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerConfigTemplate {
    pub schema_version: String,
    pub template_kind: String,
    pub solution: SolutionIdentity,
    pub optional_components: BTreeSet<String>,
    pub model_slots: BTreeSet<String>,
    pub connector_slots: BTreeSet<String>,
    pub preferences: BTreeMap<String, Preference>,
    pub memory_spaces: BTreeMap<String, MemorySpace>,
    pub requires_customer_configuration: bool,
}

/// A value-free review summary. It is not permission to apply either change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerConfigChanges {
    pub upstream_changed: bool,
    pub customer_fields: BTreeSet<String>,
}
