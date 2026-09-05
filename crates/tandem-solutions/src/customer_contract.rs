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

pub struct PreparedCustomerConfig {
    pub request: InstallRequest,
    /// Pass this narrowed policy to the existing resolver. It intersects again
    /// with the blueprint. Never substitute customer policy for host policy.
    pub deployment_policy: Constraints,
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
    pub requires_customer_configuration: bool,
}

/// A value-free review summary. It is not permission to apply either change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomerConfigChanges {
    pub upstream_changed: bool,
    pub customer_fields: BTreeSet<String>,
}
