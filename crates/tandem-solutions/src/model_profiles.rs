//! Logical model selection over current host-approved facts. This module does
//! not grant access, perform provider requests, or own the shared spend ledger.
use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::DataClass;

use crate::{canonical_json, sha256, Constraints, LockedModelBinding, SolutionError};

fn blocked(code: &str, path: &str) -> SolutionError {
    SolutionError::new(code, path, "No currently compliant model route")
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ModelModality {
    Text,
    Image,
    Audio,
    Video,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileLimits {
    pub max_input_tokens: u64,
    pub max_output_tokens: u32,
    pub max_request_cost_microusd: u64,
    pub max_latency_ms: u64,
    /// Counts route selection evaluations; physical requests use the spend ledger.
    pub max_route_evaluations: u32,
}

impl ModelProfileLimits {
    fn narrow(&self, other: &Self) -> Self {
        Self {
            max_input_tokens: self.max_input_tokens.min(other.max_input_tokens),
            max_output_tokens: self.max_output_tokens.min(other.max_output_tokens),
            max_request_cost_microusd: self
                .max_request_cost_microusd
                .min(other.max_request_cost_microusd),
            max_latency_ms: self.max_latency_ms.min(other.max_latency_ms),
            max_route_evaluations: self.max_route_evaluations.min(other.max_route_evaluations),
        }
    }

    fn validate(&self, path: &str) -> Result<(), SolutionError> {
        if self.max_input_tokens == 0
            || self.max_output_tokens == 0
            || self.max_latency_ms == 0
            || !(1..=16).contains(&self.max_route_evaluations)
        {
            return Err(blocked("invalid_model_limits", path));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProfile {
    pub required_modalities: BTreeSet<ModelModality>,
    pub requires_tool_use: bool,
    pub limits: ModelProfileLimits,
    /// Logical class IDs. Concrete provider/model IDs remain customer bindings.
    pub fallbacks: Vec<String>,
    pub escalations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileCatalog {
    #[schemars(schema_with = "schema_version")]
    pub schema_version: String,
    pub default_class: String,
    pub profiles: BTreeMap<String, ModelProfile>,
}

fn schema_version(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    schemars::schema::SchemaObject {
        instance_type: Some(schemars::schema::InstanceType::String.into()),
        enum_values: Some(vec![serde_json::Value::String("1".into())]),
        ..Default::default()
    }
    .into()
}

pub fn model_profile_schema() -> schemars::schema::RootSchema {
    schemars::schema_for!(ModelProfileCatalog)
}

pub fn parse_model_profiles(input: &str) -> Result<ModelProfileCatalog, SolutionError> {
    if input.len() > crate::MAX_BLUEPRINT_BYTES {
        return Err(blocked("model_profile_size_limit", "model_profiles"));
    }
    let value: serde_yaml::Value = serde_yaml::from_str(input)
        .map_err(|_| blocked("invalid_model_profiles", "model_profiles"))?;
    let catalog: ModelProfileCatalog = serde_yaml::from_value(value)
        .map_err(|_| blocked("invalid_model_profiles", "model_profiles"))?;
    catalog.validate()?;
    Ok(catalog)
}

impl ModelProfileCatalog {
    pub fn validate(&self) -> Result<(), SolutionError> {
        if self.schema_version != "1"
            || self.profiles.is_empty()
            || self.profiles.len() > 64
            || !self.profiles.contains_key(&self.default_class)
        {
            return Err(blocked("invalid_model_profiles", "model_profiles"));
        }
        for (class, profile) in &self.profiles {
            crate::validate::identifier(class, "model_profiles.class")?;
            profile.limits.validate(class)?;
            if profile.required_modalities.is_empty() {
                return Err(blocked("missing_model_modality", class));
            }
            for edges in [&profile.fallbacks, &profile.escalations] {
                let mut seen = BTreeSet::new();
                if edges.len() > 16
                    || edges.iter().any(|target| {
                        target == class
                            || !self.profiles.contains_key(target)
                            || !seen.insert(target)
                    })
                {
                    return Err(blocked("invalid_model_transition", class));
                }
            }
        }
        // Cross-class cycles are allowed in the catalog (e.g. escalate to
        // reasoning, fall back to economy), but a run cannot revisit a class.
        Ok(())
    }
}

/// Host-approved upper rates, shared by selection and runtime reconciliation.
/// Cache categories must fit the input rate; this is not a billing guarantee.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProfilePrice {
    pub input_microusd_per_million: u64,
    pub output_microusd_per_million: u64,
    pub request_microusd: u64,
    pub valid_until_ms: u64,
}

impl ModelProfilePrice {
    pub fn cost(&self, input: u64, output: u64) -> Result<u64, SolutionError> {
        let tokens = |count: u64, rate: u64| {
            u128::from(count)
                .checked_mul(u128::from(rate))
                .and_then(|value| value.checked_add(999_999))
                .and_then(|value| u64::try_from(value / 1_000_000).ok())
                .ok_or_else(|| blocked("model_price_overflow", "model.price"))
        };
        let input_cost = tokens(input, self.input_microusd_per_million)?;
        let output_cost = tokens(output, self.output_microusd_per_million)?;
        self.request_microusd
            .checked_add(input_cost)
            .and_then(|value| value.checked_add(output_cost))
            .ok_or_else(|| blocked("model_price_overflow", "model.price"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDataPolicy {
    pub data_class: DataClass,
    pub allowed_providers: BTreeSet<String>,
    pub allowed_processing_regions: BTreeSet<String>,
    pub max_retention_hours: u64,
    pub allow_network: bool,
}

/// Independently current facts from an authorized host resolver. Do not
/// deserialize this struct from customer config, an agent, or a request body.
#[derive(Clone, Debug)]
pub struct ModelRouteFacts {
    pub binding: LockedModelBinding,
    pub binding_revision: String,
    pub modalities: BTreeSet<ModelModality>,
    pub supports_tool_use: bool,
    pub processing_regions: BTreeSet<String>,
    pub retention_hours: Option<u64>,
    /// Must come from current availability evidence, not catalog presence.
    pub available_until_ms: Option<u64>,
    pub price: Option<ModelProfilePrice>,
}

/// Trusted per-run inputs. The controller must persist selection history and
/// ceilings with root-generation CAS; cloning a prior input is not replay safety.
pub struct ModelProfileResolutionInput<'a> {
    pub catalog: &'a ModelProfileCatalog,
    pub requested_class: Option<&'a str>,
    pub escalation_from: Option<&'a str>,
    pub escalation_reason: Option<&'a str>,
    pub customer_bindings: &'a BTreeMap<String, String>,
    pub current_routes: &'a BTreeMap<String, ModelRouteFacts>,
    pub constraints: &'a Constraints,
    pub data_policies: &'a [ModelDataPolicy],
    pub data_classes: &'a [DataClass],
    pub modalities: &'a BTreeSet<ModelModality>,
    pub uses_tools: bool,
    /// A supported host upper bound; never infer this from prompt character count.
    pub maximum_input_tokens: u64,
    pub maximum_output_tokens: u32,
    pub inherited_limits: &'a ModelProfileLimits,
    /// The preceding decision's complete `selected_path`, including fallbacks.
    pub previously_selected: &'a [String],
    pub previous_evaluations: u32,
    pub expected_catalog_sha256: Option<&'a str>,
    pub started_at_ms: u64,
    pub now_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelProfileDecision {
    pub requested_class: String,
    pub selected_class: String,
    pub binding: LockedModelBinding,
    pub binding_revision: String,
    pub catalog_sha256: String,
    pub effective_limits: ModelProfileLimits,
    pub maximum_cost_microusd: u64,
    pub deadline_ms: u64,
    pub evaluated_classes: Vec<String>,
    /// Persist this complete path for the next resolution, including classes
    /// whose constraints narrowed a fallback before a route was selected.
    pub selected_path: Vec<String>,
    pub route_evaluations: u32,
    pub escalation_reason: Option<String>,
}

#[derive(Clone)]
struct RequiredCapabilities {
    modalities: BTreeSet<ModelModality>,
    tools: bool,
}

impl RequiredCapabilities {
    fn include(&mut self, profile: &ModelProfile) {
        self.modalities
            .extend(profile.required_modalities.iter().copied());
        self.tools |= profile.requires_tool_use;
    }
}

fn candidate(
    class: &str,
    route: &ModelRouteFacts,
    required: &RequiredCapabilities,
    limits: &ModelProfileLimits,
    input: &ModelProfileResolutionInput<'_>,
) -> Result<u64, SolutionError> {
    let deny = |code| blocked(code, class);
    if route.binding_revision.is_empty()
        || route.binding_revision.len() > 256
        || route
            .available_until_ms
            .is_none_or(|until| input.now_ms > until)
    {
        return Err(deny("model_unavailable"));
    }
    let binding = &route.binding.binding;
    if !input
        .constraints
        .allowed_providers
        .contains(&binding.provider)
        || (binding.uses_network && !input.constraints.allow_network_egress)
    {
        return Err(deny("model_provider_disallowed"));
    }
    if !required.modalities.is_subset(&route.modalities)
        || !input.modalities.is_subset(&route.modalities)
        || ((input.uses_tools || required.tools) && !route.supports_tool_use)
    {
        return Err(deny("model_capability_mismatch"));
    }
    if input.maximum_input_tokens > limits.max_input_tokens
        || input.maximum_output_tokens > limits.max_output_tokens
        || input
            .maximum_input_tokens
            .checked_add(u64::from(input.maximum_output_tokens))
            .is_none_or(|tokens| tokens > input.constraints.max_tokens_per_run)
    {
        return Err(deny("model_token_ceiling"));
    }
    for data_class in input.data_classes {
        let mut matching = input
            .data_policies
            .iter()
            .filter(|policy| policy.data_class == *data_class);
        let policy = matching
            .next()
            .ok_or_else(|| deny("model_data_policy_missing"))?;
        if matching.next().is_some() {
            return Err(deny("model_data_policy_ambiguous"));
        }
        if !policy.allowed_providers.contains(&binding.provider)
            || (binding.uses_network && !policy.allow_network)
            || route.processing_regions.is_empty()
            || !route
                .processing_regions
                .is_subset(&policy.allowed_processing_regions)
            || route
                .retention_hours
                .is_none_or(|hours| hours > policy.max_retention_hours)
        {
            return Err(deny("model_data_policy_denied"));
        }
    }
    let price = route
        .price
        .as_ref()
        .ok_or_else(|| deny("model_price_unknown"))?;
    if input.now_ms > price.valid_until_ms {
        return Err(deny("model_price_expired"));
    }
    let cost = price.cost(
        input.maximum_input_tokens,
        u64::from(input.maximum_output_tokens),
    )?;
    if cost > limits.max_request_cost_microusd || cost > input.constraints.max_daily_cost_microusd {
        return Err(deny("model_cost_ceiling"));
    }
    Ok(cost)
}

pub fn resolve_model_profile(
    input: ModelProfileResolutionInput<'_>,
) -> Result<ModelProfileDecision, SolutionError> {
    input.catalog.validate()?;
    input.inherited_limits.validate("model.inherited_limits")?;
    let catalog_sha256 = sha256(&canonical_json(input.catalog)?);
    if input.data_classes.is_empty()
        || input.modalities.is_empty()
        || input.maximum_input_tokens == 0
        || input.maximum_output_tokens == 0
        || input.now_ms < input.started_at_ms
        || input.previously_selected.len() > 16
        || input.previous_evaluations < input.previously_selected.len() as u32
    {
        return Err(blocked("invalid_model_request", "model.request"));
    }
    if (!input.previously_selected.is_empty()
        && input.expected_catalog_sha256 != Some(catalog_sha256.as_str()))
        || input
            .expected_catalog_sha256
            .is_some_and(|expected| expected != catalog_sha256)
    {
        return Err(blocked("model_catalog_changed", "model_profiles"));
    }
    let requested = input
        .requested_class
        .unwrap_or(&input.catalog.default_class);
    let origin = input
        .catalog
        .profiles
        .get(requested)
        .ok_or_else(|| blocked("model_class_unknown", requested))?;
    let escalation_reason = match input.escalation_from {
        Some(from) => {
            let source = input
                .catalog
                .profiles
                .get(from)
                .ok_or_else(|| blocked("model_class_unknown", from))?;
            let reason = input
                .escalation_reason
                .map(str::trim)
                .filter(|reason| !reason.is_empty() && reason.len() <= 512)
                .ok_or_else(|| blocked("model_escalation_reason_required", requested))?;
            if !source.escalations.iter().any(|target| target == requested)
                || input.previously_selected.last().map(String::as_str) != Some(from)
            {
                return Err(blocked("model_escalation_denied", requested));
            }
            Some(reason.to_string())
        }
        None if input.escalation_reason.is_some() => {
            return Err(blocked("model_escalation_denied", requested))
        }
        None => {
            if let Some(previous) = input.previously_selected.last() {
                let source = input
                    .catalog
                    .profiles
                    .get(previous)
                    .ok_or_else(|| blocked("model_class_unknown", previous))?;
                if !source.fallbacks.iter().any(|target| target == requested) {
                    return Err(blocked("model_fallback_denied", requested));
                }
            }
            None
        }
    };
    let mut limits = input.inherited_limits.narrow(&origin.limits);
    let mut required = RequiredCapabilities {
        modalities: origin.required_modalities.clone(),
        tools: origin.requires_tool_use,
    };
    // Every earlier class continues to narrow the run, even when the caller
    // supplies an overly broad inherited ceiling during fallback/escalation.
    let mut visited = Vec::new();
    for previous in input.previously_selected {
        if visited.contains(previous) {
            return Err(blocked("model_route_cycle", previous));
        }
        visited.push(previous.clone());
        let previous_profile = input
            .catalog
            .profiles
            .get(previous)
            .ok_or_else(|| blocked("model_class_unknown", previous))?;
        limits = limits.narrow(&previous_profile.limits);
        required.include(previous_profile);
    }
    let mut stack = vec![(requested.to_string(), limits, required, visited)];
    let mut evaluated = Vec::new();
    let mut evaluations = input.previous_evaluations;
    let mut last_error = blocked("model_no_compliant_route", requested);
    while let Some((class, inherited, mut required, mut visited)) = stack.pop() {
        if visited.contains(&class) {
            last_error = blocked("model_route_cycle", &class);
            continue;
        }
        visited.push(class.clone());
        let profile = &input.catalog.profiles[&class];
        let limits = inherited.narrow(&profile.limits);
        required.include(profile);
        evaluations = evaluations
            .checked_add(1)
            .ok_or_else(|| blocked("model_route_limit", &class))?;
        if evaluations > limits.max_route_evaluations {
            return Err(blocked("model_route_limit", &class));
        }
        let deadline = input
            .started_at_ms
            .checked_add(limits.max_latency_ms)
            .ok_or_else(|| blocked("model_latency_limit", &class))?;
        if input.now_ms >= deadline {
            return Err(blocked("model_latency_limit", &class));
        }
        evaluated.push(class.clone());
        let route = input
            .customer_bindings
            .get(&class)
            .and_then(|id| input.current_routes.get(id));
        let result = route
            .ok_or_else(|| blocked("model_binding_unavailable", &class))
            .and_then(|route| {
                if input.customer_bindings[&class] != route.binding.binding_id {
                    return Err(blocked("model_binding_mismatch", &class));
                }
                candidate(&class, route, &required, &limits, &input)
            });
        match result {
            Ok(cost) => {
                let route = route.expect("successful candidate has a route");
                return Ok(ModelProfileDecision {
                    requested_class: requested.into(),
                    selected_class: class,
                    binding: route.binding.clone(),
                    binding_revision: route.binding_revision.clone(),
                    catalog_sha256,
                    effective_limits: limits,
                    maximum_cost_microusd: cost,
                    deadline_ms: deadline,
                    evaluated_classes: evaluated,
                    selected_path: visited,
                    route_evaluations: evaluations,
                    escalation_reason,
                });
            }
            Err(error) => last_error = error,
        }
        for fallback in profile.fallbacks.iter().rev() {
            stack.push((
                fallback.clone(),
                limits.clone(),
                required.clone(),
                visited.clone(),
            ));
        }
    }
    Err(last_error)
}

#[cfg(test)]
#[path = "model_profiles_tests.rs"]
mod tests;
