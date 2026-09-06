use super::*;
use crate::ModelBinding;

struct Fixture {
    catalog: ModelProfileCatalog,
    mappings: BTreeMap<String, String>,
    routes: BTreeMap<String, ModelRouteFacts>,
    constraints: Constraints,
    policies: Vec<ModelDataPolicy>,
    classes: Vec<DataClass>,
    modalities: BTreeSet<ModelModality>,
    limits: ModelProfileLimits,
}

impl Fixture {
    fn new() -> Self {
        let limits = ModelProfileLimits {
            max_input_tokens: 100,
            max_output_tokens: 20,
            max_request_cost_microusd: 100,
            max_latency_ms: 1000,
            max_route_evaluations: 6,
        };
        let profile = ModelProfile {
            required_modalities: [ModelModality::Text].into(),
            requires_tool_use: false,
            limits: limits.clone(),
            fallbacks: vec![],
            escalations: vec![],
        };
        let mut catalog = ModelProfileCatalog {
            schema_version: "1".into(),
            default_class: "economy".into(),
            profiles: ["economy", "reasoning", "offline"]
                .into_iter()
                .map(|id| (id.into(), profile.clone()))
                .collect(),
        };
        catalog.profiles.get_mut("economy").unwrap().fallbacks = vec!["offline".into()];
        catalog.profiles.get_mut("economy").unwrap().escalations = vec!["reasoning".into()];
        catalog.profiles.get_mut("reasoning").unwrap().fallbacks = vec!["economy".into()];
        let mut mappings = BTreeMap::new();
        let mut routes = BTreeMap::new();
        for (class, provider, network) in [
            ("economy", "provider-a", true),
            ("reasoning", "provider-b", true),
            ("offline", "local", false),
        ] {
            let id = format!("binding-{class}");
            mappings.insert(class.into(), id.clone());
            routes.insert(
                id.clone(),
                ModelRouteFacts {
                    binding: LockedModelBinding {
                        binding_id: id,
                        binding: ModelBinding {
                            provider: provider.into(),
                            model: format!("model-{class}"),
                            credential_ref: format!("account-{class}"),
                            uses_network: network,
                        },
                    },
                    binding_revision: sha256(class.as_bytes()),
                    modalities: [ModelModality::Text].into(),
                    supports_tool_use: false,
                    processing_regions: [if network { "eu".into() } else { "local".into() }].into(),
                    retention_hours: Some(1),
                    available_until_ms: Some(1000),
                    price: Some(ModelProfilePrice {
                        input_microusd_per_million: if network { 1_000_000 } else { 0 },
                        output_microusd_per_million: if network { 1_000_000 } else { 0 },
                        request_microusd: 0,
                        valid_until_ms: 1000,
                    }),
                },
            );
        }
        let providers: BTreeSet<String> = ["provider-a", "provider-b", "local"]
            .into_iter()
            .map(str::to_string)
            .collect();
        Self {
            catalog,
            mappings,
            routes,
            constraints: Constraints {
                allowed_providers: providers.clone(),
                allow_network_egress: true,
                max_tokens_per_run: 1000,
                max_concurrent_runs: 2,
                max_daily_cost_microusd: 1000,
            },
            policies: vec![ModelDataPolicy {
                data_class: DataClass::Confidential,
                allowed_providers: providers,
                allowed_processing_regions: ["eu".into(), "local".into()].into(),
                max_retention_hours: 24,
                allow_network: true,
            }],
            classes: vec![DataClass::Confidential],
            modalities: [ModelModality::Text].into(),
            limits,
        }
    }

    fn input(&self) -> ModelProfileResolutionInput<'_> {
        ModelProfileResolutionInput {
            catalog: &self.catalog,
            requested_class: None,
            escalation_from: None,
            escalation_reason: None,
            customer_bindings: &self.mappings,
            current_routes: &self.routes,
            constraints: &self.constraints,
            data_policies: &self.policies,
            data_classes: &self.classes,
            modalities: &self.modalities,
            uses_tools: false,
            maximum_input_tokens: 10,
            maximum_output_tokens: 5,
            inherited_limits: &self.limits,
            previously_selected: &[],
            previous_evaluations: 0,
            expected_catalog_sha256: None,
            started_at_ms: 100,
            now_ms: 100,
        }
    }
}

#[test]
fn model_profile_default_is_vendor_independent_and_current_binding_is_versioned() {
    let mut f = Fixture::new();
    let first = resolve_model_profile(f.input()).unwrap();
    assert_eq!(first.selected_class, "economy");
    assert_eq!(first.maximum_cost_microusd, 15);
    let catalog_hash = first.catalog_sha256;
    f.mappings
        .insert("economy".into(), "binding-reasoning".into());
    let second = resolve_model_profile(f.input()).unwrap();
    assert_eq!(second.selected_class, "economy");
    assert_eq!(second.binding.binding.provider, "provider-b");
    assert_eq!(
        second.catalog_sha256, catalog_hash,
        "worker profile does not contain concrete model names"
    );
    assert_ne!(second.binding_revision, first.binding_revision);
}

#[test]
fn model_profile_unavailable_primary_uses_only_a_compliant_fallback() {
    let mut f = Fixture::new();
    f.routes
        .get_mut("binding-economy")
        .unwrap()
        .available_until_ms = None;
    let selected = resolve_model_profile(f.input()).unwrap();
    assert_eq!(selected.selected_class, "offline");
    assert_eq!(selected.evaluated_classes, ["economy", "offline"]);
    assert_eq!(
        selected.maximum_cost_microusd, 0,
        "explicit current zero price"
    );
    f.policies[0].allowed_providers.remove("local");
    assert_eq!(
        resolve_model_profile(f.input()).unwrap_err().code,
        "model_data_policy_denied"
    );
}

#[test]
fn model_profile_fallback_preserves_original_modality_and_tool_requirements() {
    let mut f = Fixture::new();
    let economy = f.catalog.profiles.get_mut("economy").unwrap();
    economy.required_modalities.insert(ModelModality::Image);
    economy.requires_tool_use = true;
    assert_eq!(
        resolve_model_profile(f.input()).unwrap_err().code,
        "model_capability_mismatch"
    );
    let fallback = f.routes.get_mut("binding-offline").unwrap();
    fallback.modalities.insert(ModelModality::Image);
    fallback.supports_tool_use = true;
    assert_eq!(
        resolve_model_profile(f.input()).unwrap().selected_class,
        "offline"
    );
}

#[test]
fn model_profile_checks_every_data_class_and_rejects_unknown_region_or_retention() {
    for fault in [
        "class",
        "duplicate-policy",
        "region",
        "retention",
        "network",
        "provider",
    ] {
        let mut f = Fixture::new();
        f.catalog
            .profiles
            .get_mut("economy")
            .unwrap()
            .fallbacks
            .clear();
        match fault {
            "class" => f.classes.push(DataClass::Regulated),
            "duplicate-policy" => f.policies.push(f.policies[0].clone()),
            "region" => {
                f.routes
                    .get_mut("binding-economy")
                    .unwrap()
                    .processing_regions
                    .insert("unapproved".into());
            }
            "retention" => f.routes.get_mut("binding-economy").unwrap().retention_hours = None,
            "network" => f.policies[0].allow_network = false,
            "provider" => {
                f.constraints.allowed_providers.remove("provider-a");
            }
            _ => unreachable!(),
        }
        assert!(resolve_model_profile(f.input()).is_err(), "{fault}");
    }
    let mut f = Fixture::new();
    f.classes.clear();
    assert_eq!(
        resolve_model_profile(f.input()).unwrap_err().code,
        "invalid_model_request"
    );
}

#[test]
fn model_profile_unknown_expired_or_overflowing_price_is_never_free() {
    for fault in [
        "unknown", "expired", "overflow", "cost", "tokens", "latency",
    ] {
        let mut f = Fixture::new();
        f.catalog
            .profiles
            .get_mut("economy")
            .unwrap()
            .fallbacks
            .clear();
        match fault {
            "unknown" => f.routes.get_mut("binding-economy").unwrap().price = None,
            "expired" => {
                f.routes
                    .get_mut("binding-economy")
                    .unwrap()
                    .price
                    .as_mut()
                    .unwrap()
                    .valid_until_ms = 99
            }
            "overflow" => {
                f.routes
                    .get_mut("binding-economy")
                    .unwrap()
                    .price
                    .as_mut()
                    .unwrap()
                    .request_microusd = u64::MAX
            }
            "cost" => f.limits.max_request_cost_microusd = 14,
            "tokens" => f.constraints.max_tokens_per_run = 14,
            "latency" => f.limits.max_latency_ms = 1,
            _ => unreachable!(),
        }
        let mut input = f.input();
        if fault == "latency" {
            input.now_ms = 101;
        }
        assert!(resolve_model_profile(input).is_err(), "{fault}");
    }
}

#[test]
fn model_profile_escalation_requires_an_edge_and_reason_and_preserves_prior_ceiling() {
    let mut f = Fixture::new();
    f.catalog
        .profiles
        .get_mut("reasoning")
        .unwrap()
        .limits
        .max_output_tokens = 1000;
    let first = resolve_model_profile(f.input()).unwrap();
    let history = vec!["economy".to_string()];
    let mut input = f.input();
    input.requested_class = Some("reasoning");
    input.escalation_from = Some("economy");
    input.escalation_reason = Some("low confidence in the first route");
    input.previously_selected = &history;
    input.previous_evaluations = 1;
    input.expected_catalog_sha256 = Some(&first.catalog_sha256);
    let result = resolve_model_profile(input).unwrap();
    assert_eq!(result.selected_class, "reasoning");
    assert_eq!(result.effective_limits.max_output_tokens, 20);
    assert_eq!(result.route_evaluations, 2);
    assert!(result.escalation_reason.unwrap().contains("low confidence"));
    for invalid in ["missing-reason", "unapproved-target", "no-edge"] {
        let mut input = f.input();
        input.requested_class = Some(if invalid == "unapproved-target" {
            "offline"
        } else {
            "reasoning"
        });
        input.escalation_from = if invalid == "no-edge" {
            None
        } else {
            Some("economy")
        };
        input.escalation_reason = if invalid == "missing-reason" || invalid == "no-edge" {
            None
        } else {
            Some("complexity")
        };
        input.previously_selected = &history;
        input.previous_evaluations = 1;
        input.expected_catalog_sha256 = Some(&first.catalog_sha256);
        assert!(resolve_model_profile(input).is_err(), "{invalid}");
    }
}

#[test]
fn model_profile_cycles_and_exhausted_history_stop_without_resetting_the_budget() {
    let mut f = Fixture::new();
    f.catalog.profiles.get_mut("economy").unwrap().fallbacks = vec!["reasoning".into()];
    f.routes
        .get_mut("binding-economy")
        .unwrap()
        .available_until_ms = None;
    f.routes
        .get_mut("binding-reasoning")
        .unwrap()
        .available_until_ms = None;
    assert_eq!(
        resolve_model_profile(f.input()).unwrap_err().code,
        "model_route_cycle"
    );
    f.limits.max_route_evaluations = 1;
    assert_eq!(
        resolve_model_profile(f.input()).unwrap_err().code,
        "model_route_limit"
    );
    let f = Fixture::new();
    let digest = sha256(&canonical_json(&f.catalog).unwrap());
    let history = vec!["economy".into(), "reasoning".into()];
    let mut input = f.input();
    input.requested_class = Some("economy");
    input.previously_selected = &history;
    input.previous_evaluations = 2;
    input.expected_catalog_sha256 = Some(&digest);
    assert_eq!(
        resolve_model_profile(input).unwrap_err().code,
        "model_route_cycle"
    );
    let mut input = f.input();
    input.previous_evaluations = 6;
    assert_eq!(
        resolve_model_profile(input).unwrap_err().code,
        "model_route_limit"
    );
}

#[test]
fn model_profile_graph_can_consider_a_shared_fallback_under_distinct_narrowing_paths() {
    let mut f = Fixture::new();
    f.routes
        .get_mut("binding-economy")
        .unwrap()
        .available_until_ms = None;
    f.catalog.profiles.get_mut("economy").unwrap().fallbacks =
        vec!["reasoning".into(), "offline".into()];
    let reasoning = f.catalog.profiles.get_mut("reasoning").unwrap();
    reasoning.required_modalities.insert(ModelModality::Image);
    reasoning.fallbacks = vec!["offline".into()];
    let result = resolve_model_profile(f.input()).unwrap();
    assert_eq!(result.selected_class, "offline");
    assert_eq!(
        result.evaluated_classes,
        ["economy", "reasoning", "offline", "offline"]
    );
}

#[test]
fn model_profile_changed_catalog_cannot_continue_an_existing_route_history() {
    let mut f = Fixture::new();
    let first = resolve_model_profile(f.input()).unwrap();
    f.catalog
        .profiles
        .get_mut("economy")
        .unwrap()
        .limits
        .max_request_cost_microusd += 1;
    let history = vec!["economy".into()];
    let mut input = f.input();
    input.requested_class = Some("offline");
    input.previously_selected = &history;
    input.previous_evaluations = 1;
    input.expected_catalog_sha256 = Some(&first.catalog_sha256);
    assert_eq!(
        resolve_model_profile(input).unwrap_err().code,
        "model_catalog_changed"
    );
}

#[test]
fn model_profile_fallback_path_survives_later_escalation() {
    let mut f = Fixture::new();
    f.catalog
        .profiles
        .get_mut("economy")
        .unwrap()
        .required_modalities
        .insert(ModelModality::Image);
    f.catalog.profiles.get_mut("offline").unwrap().escalations = vec!["reasoning".into()];
    f.routes
        .get_mut("binding-offline")
        .unwrap()
        .modalities
        .insert(ModelModality::Image);
    let first = resolve_model_profile(f.input()).unwrap();
    assert_eq!(first.selected_path, ["economy", "offline"]);
    let mut next = f.input();
    next.requested_class = Some("reasoning");
    next.escalation_from = Some("offline");
    next.escalation_reason = Some("review the first result");
    next.previously_selected = &first.selected_path;
    next.previous_evaluations = first.route_evaluations;
    next.expected_catalog_sha256 = Some(&first.catalog_sha256);
    // Reasoning lacks the image capability required by the original profile;
    // its economy fallback cannot revisit the already selected path either.
    assert!(resolve_model_profile(next).is_err());
}

#[test]
fn model_profile_parser_and_schema_preserve_version_and_reject_duplicate_keys() {
    let example = include_str!("../fixtures/model-profiles/text-default.json");
    let catalog = parse_model_profiles(example).unwrap();
    assert_eq!(catalog.default_class, "economy");
    assert_eq!(
        catalog.profiles["economy"].limits.max_request_cost_microusd,
        0
    );
    let duplicate = example.replace(
        "\"schema_version\": \"1\",",
        "\"schema_version\": \"1\", \"schema_version\": \"1\",",
    );
    assert!(parse_model_profiles(&duplicate).is_err());
    let mut value = serde_json::to_value(&catalog).unwrap();
    value["unapproved"] = serde_json::json!(true);
    assert!(parse_model_profiles(&value.to_string()).is_err());
    let schema = serde_json::to_value(model_profile_schema()).unwrap();
    assert_eq!(
        schema["properties"]["schema_version"]["enum"],
        serde_json::json!(["1"])
    );
    assert_eq!(schema["additionalProperties"], false);
}
