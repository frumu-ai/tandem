use super::*;
use std::collections::{BTreeMap, BTreeSet};
use tandem_enterprise_contract::DataClass;
use tandem_solutions::{
    ModelDataPolicy, ModelModality, ModelProfile, ModelProfileCatalog, ModelProfileLimits,
    ModelProfilePrice, ModelProfileResolutionInput, ModelRouteFacts,
};

struct ProfileFixture {
    catalog: ModelProfileCatalog,
    routes: BTreeMap<String, ModelRouteFacts>,
    policies: Vec<ModelDataPolicy>,
    classes: Vec<DataClass>,
    modalities: BTreeSet<ModelModality>,
    limits: ModelProfileLimits,
    // Deliberately wrong caller values: the store must use the installation.
    caller_bindings: BTreeMap<String, String>,
    caller_constraints: tandem_solutions::Constraints,
    forged_path: Vec<String>,
}

impl ProfileFixture {
    fn new(approved: &ApprovedSolutionProviderCharge) -> Self {
        let limits = ModelProfileLimits {
            max_input_tokens: 20,
            max_output_tokens: 10,
            max_request_cost_microusd: 50,
            max_latency_ms: 1000,
            max_route_evaluations: 3,
        };
        let binding = approved.binding.clone();
        let provider = binding.binding.provider.clone();
        let id = binding.binding_id.clone();
        Self {
            catalog: ModelProfileCatalog {
                schema_version: "1".into(),
                default_class: "economy".into(),
                profiles: [(
                    "economy".into(),
                    ModelProfile {
                        required_modalities: [ModelModality::Text].into(),
                        requires_tool_use: false,
                        limits: limits.clone(),
                        fallbacks: Vec::new(),
                        escalations: Vec::new(),
                    },
                )]
                .into(),
            },
            routes: [(
                id,
                ModelRouteFacts {
                    binding,
                    binding_revision: sha256(b"approved-route"),
                    modalities: [ModelModality::Text].into(),
                    supports_tool_use: false,
                    processing_regions: ["eu".into()].into(),
                    retention_hours: Some(1),
                    available_until_ms: Some(1900),
                    price: Some(ModelProfilePrice {
                        input_microusd_per_million: 1_000_000,
                        output_microusd_per_million: 1_000_000,
                        request_microusd: 0,
                        valid_until_ms: 1900,
                    }),
                },
            )]
            .into(),
            policies: vec![ModelDataPolicy {
                data_class: DataClass::Confidential,
                allowed_providers: [provider].into(),
                allowed_processing_regions: ["eu".into()].into(),
                max_retention_hours: 2,
                allow_network: true,
            }],
            classes: vec![DataClass::Confidential],
            modalities: [ModelModality::Text].into(),
            limits,
            caller_bindings: [("economy".into(), "caller-picked-binding".into())].into(),
            caller_constraints: tandem_solutions::Constraints {
                allowed_providers: BTreeSet::new(),
                allow_network_egress: false,
                max_tokens_per_run: 1,
                max_concurrent_runs: 1,
                max_daily_cost_microusd: 1,
            },
            forged_path: vec!["forged-history".into()],
        }
    }

    fn input(&self) -> ModelProfileResolutionInput<'_> {
        ModelProfileResolutionInput {
            catalog: &self.catalog,
            requested_class: None,
            escalation_from: None,
            escalation_reason: None,
            customer_bindings: &self.caller_bindings,
            current_routes: &self.routes,
            constraints: &self.caller_constraints,
            data_policies: &self.policies,
            data_classes: &self.classes,
            modalities: &self.modalities,
            uses_tools: false,
            maximum_input_tokens: 20,
            maximum_output_tokens: 10,
            inherited_limits: &self.limits,
            previously_selected: &self.forged_path,
            previous_evaluations: 999,
            expected_catalog_sha256: Some("forged-catalog"),
            started_at_ms: 1,
            now_ms: 1,
        }
    }
}

fn select(
    store: &OrchestrationStateStore,
    approved: &ApprovedSolutionProviderCharge,
    profile: &ProfileFixture,
    operation_id: &str,
    selection_id: &str,
    now_ms: u64,
) -> anyhow::Result<RuntimeProfileSelection> {
    store.select_runtime_model_profile(
        RuntimeProfileRequest {
            goal: SolutionGoalStart {
                verified: &approved.verified,
                scope: &approved.scope,
                configuration: &approved.configuration,
                installation_generation: approved.installation_generation,
                composition_sha256: &approved.composition_sha256,
                now_ms: 1, // ignored; the writer-lock clock is authoritative
            },
            execution: &approved.execution,
            root_run_id: &approved.root_run_id,
            operation_id,
            selection_id,
            profile: profile.input(),
        },
        || now_ms,
    )
}

#[test]
#[serial]
fn solution_profile_history_replay_restart_and_independent_operations() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = network_fixture(store, "profile-replay");
            let approved = approval(&fixture, store);
            let profile = ProfileFixture::new(&approved);
            let first =
                select(store, &approved, &profile, "worker-a", "selection-1", 1500).unwrap();
            let RuntimeProfileSelection::Selected(decision) = first else {
                panic!("selection missing")
            };
            assert_eq!(decision.selected_class, "economy");
            assert_eq!(decision.route_evaluations, 1);
            assert_eq!(decision.binding, approved.binding);
            let reopened = store.clone();
            reopened.initialize().unwrap();
            assert_eq!(
                select(
                    &reopened,
                    &approved,
                    &profile,
                    "worker-a",
                    "selection-1",
                    1500
                )
                .unwrap(),
                RuntimeProfileSelection::Replayed
            );
            assert_eq!(
                select(
                    &reopened,
                    &approved,
                    &profile,
                    "worker-b",
                    "selection-1",
                    1500
                )
                .unwrap(),
                RuntimeProfileSelection::Selected(decision)
            );
            assert_eq!(
                select(
                    &reopened,
                    &approved,
                    &profile,
                    "worker-a",
                    "selection-2",
                    1500
                )
                .unwrap(),
                RuntimeProfileSelection::Blocked("model_fallback_denied".into())
            );
            assert_eq!(
                select(
                    &reopened,
                    &approved,
                    &profile,
                    "worker-a",
                    "selection-3",
                    1500
                )
                .unwrap(),
                RuntimeProfileSelection::Blocked("model_fallback_denied".into())
            );
        });
    });
}

#[test]
#[serial]
fn solution_profile_history_failed_route_stays_terminal_and_requires_current_goal() {
    encrypted(|| {
        for_each_backend(|_, store| {
            let fixture = network_fixture(store, "profile-failure");
            let approved = approval(&fixture, store);
            let mut profile = ProfileFixture::new(&approved);
            profile
                .routes
                .values_mut()
                .next()
                .unwrap()
                .available_until_ms = Some(1400);
            assert_eq!(
                select(store, &approved, &profile, "worker-a", "selection-1", 1500).unwrap(),
                RuntimeProfileSelection::Blocked("model_unavailable".into())
            );
            profile
                .routes
                .values_mut()
                .next()
                .unwrap()
                .available_until_ms = Some(1900);
            let reopened = store.clone();
            reopened.initialize().unwrap();
            assert_eq!(
                select(
                    &reopened,
                    &approved,
                    &profile,
                    "worker-a",
                    "selection-2",
                    1500
                )
                .unwrap(),
                RuntimeProfileSelection::Blocked("model_unavailable".into())
            );
            let mut foreign = approved.clone();
            foreign.installation_generation += 1;
            assert!(select(
                &reopened,
                &foreign,
                &profile,
                "worker-b",
                "selection-1",
                1500
            )
            .is_err());
        });
    });
}
