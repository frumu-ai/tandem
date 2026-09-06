// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::BTreeMap;
use tandem_enterprise_contract::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContext, TenantContextAssertionClaims,
    VerifiedTenantContext,
};
use tandem_solutions::*;

const FIXTURE: &str = include_str!("../fixtures/company-brain-text/solution.json");

struct Fixture {
    blueprint: SolutionBlueprint,
    request: InstallRequest,
    approved_models: BTreeMap<String, ModelBinding>,
    context: VerifiedTenantContext,
    policy: Constraints,
    artifacts: BTreeMap<String, Vec<u8>>,
}

impl Fixture {
    fn new() -> Self {
        let blueprint = parse_blueprint(FIXTURE).unwrap();
        let principal = RequestPrincipal::authenticated_user("owner-a", "fixture");
        let context = TenantContextAssertionClaims::new_v1(
            "issuer",
            "runtime",
            1000,
            2000,
            "assertion-a",
            TenantContext::explicit_user_workspace(
                "org-a",
                "workspace-a",
                Some("deployment-a".into()),
                "owner-a",
            ),
            HumanActor::tandem_user("owner-a"),
            AuthorityChain::from_request(principal),
            vec!["workspace:user".into()],
        )
        .into();
        Self {
            policy: blueprint.constraints.clone(),
            blueprint,
            request: serde_json::from_str(include_str!(
                "../fixtures/company-brain-text/request.json"
            ))
            .unwrap(),
            approved_models: serde_json::from_str(include_str!(
                "../fixtures/company-brain-text/host-models.json"
            ))
            .unwrap(),
            context,
            artifacts: BTreeMap::from([
                (
                    "central-brain".into(),
                    include_bytes!("../fixtures/company-brain-text/agents/central-brain.json")
                        .to_vec(),
                ),
                (
                    "review-notes".into(),
                    include_bytes!("../fixtures/company-brain-text/routines/review-notes.json")
                        .to_vec(),
                ),
            ]),
        }
    }
    fn plan(&self) -> Result<ResolvedPlan, SolutionError> {
        self.plan_with_host_facts(None)
    }
    fn plan_with_host_facts(
        &self,
        host_facts_sha256: Option<&str>,
    ) -> Result<ResolvedPlan, SolutionError> {
        resolve(
            &self.blueprint,
            ResolutionInput {
                host_facts_sha256,
                request: &self.request,
                approved_models: &self.approved_models,
                verified_context: &self.context,
                now_ms: 1500,
                engine_version: "0.7.2",
                deployment_policy: &self.policy,
                available_deployment_requirements: &self.blueprint.deployment_requirements,
                artifacts: &self.artifacts,
            },
        )
    }
    fn hash(&self) -> String {
        self.plan().unwrap().composition_hash().unwrap()
    }
}

#[test]
fn text_fixture_reuses_agent_template_and_needs_no_optional_modules() {
    let fixture = Fixture::new();
    let agent: tandem_orchestrator::AgentTemplate =
        serde_json::from_slice(&fixture.artifacts["central-brain"]).unwrap();
    assert_eq!(agent.template_id, "central-brain");
    assert!(!agent.capabilities.net_scopes.enabled);
    assert!(agent.capabilities.tool_allowlist.is_empty());
    let plan = fixture.plan().unwrap();
    assert_eq!(plan.install_order, ["central-brain"]);
    assert!(plan.connectors.is_empty());
    assert!(plan.required_capabilities.is_empty());
    assert_eq!(plan.memory_spaces["private"], MemorySpace::PrivateUser);
    assert_eq!(fixture.hash(), fixture.hash());
}

#[test]
fn json_and_yaml_normalize_to_identical_blueprints_and_hashes() {
    let fixture = Fixture::new();
    let yaml = serde_yaml::to_string(&fixture.blueprint).unwrap();
    let reparsed = parse_blueprint(&yaml).unwrap();
    assert_eq!(
        blueprint_hash(&fixture.blueprint).unwrap(),
        blueprint_hash(&reparsed).unwrap()
    );
    // An omitted default and an explicit empty collection are equivalent.
    let explicit = FIXTURE.replace(
        "\"required\": true,",
        "\"required\": true, \"depends_on\": {},",
    );
    assert_eq!(parse_blueprint(&explicit).unwrap(), fixture.blueprint);
}

#[test]
fn parsing_rejects_unknown_versions_fields_and_duplicate_keys() {
    assert_eq!(
        parse_blueprint(&FIXTURE.replace("\"schema_version\": \"1\"", "\"schema_version\": \"2\""))
            .unwrap_err()
            .code,
        "unsupported_schema"
    );
    let with_secret = FIXTURE.replacen('{', "{\"api_key\":\"do-not-include\",", 1);
    assert_eq!(
        parse_blueprint(&with_secret).unwrap_err().code,
        "invalid_schema"
    );
    let duplicate = FIXTURE.replacen(
        "\"required\": true",
        "\"required\": false, \"required\": true",
        1,
    );
    assert_eq!(parse_blueprint(&duplicate).unwrap_err().code, "parse_error");
    let duplicate_component = FIXTURE.replacen(
        "\"central-brain\": {",
        "\"central-brain\": {}, \"central-brain\": {",
        1,
    );
    assert_eq!(
        parse_blueprint(&duplicate_component).unwrap_err().code,
        "parse_error"
    );
}

#[test]
fn missing_cyclic_and_incompatible_dependencies_have_actionable_errors() {
    let mut fixture = Fixture::new();
    fixture
        .blueprint
        .components
        .get_mut("central-brain")
        .unwrap()
        .depends_on
        .insert("missing".into(), "*".into());
    assert_eq!(fixture.plan().unwrap_err().code, "missing_component");
    fixture
        .blueprint
        .components
        .get_mut("central-brain")
        .unwrap()
        .depends_on = BTreeMap::from([("review-notes".into(), "*".into())]);
    assert_eq!(fixture.plan().unwrap_err().code, "dependency_cycle");
    fixture
        .blueprint
        .components
        .get_mut("central-brain")
        .unwrap()
        .depends_on
        .clear();
    fixture
        .blueprint
        .components
        .get_mut("review-notes")
        .unwrap()
        .depends_on
        .insert("central-brain".into(), "^2.0.0".into());
    let error = fixture.plan().unwrap_err();
    assert_eq!(error.code, "incompatible_component");
    assert!(error
        .path
        .ends_with("review-notes.depends_on.central-brain"));
}

#[test]
fn optional_selection_dependency_closure_and_conflicts_are_deterministic() {
    let mut fixture = Fixture::new();
    fixture
        .request
        .optional_components
        .insert("review-notes".into());
    fixture
        .blueprint
        .components
        .get_mut("central-brain")
        .unwrap()
        .required = false;
    assert_eq!(
        fixture.plan().unwrap().install_order,
        ["central-brain", "review-notes"]
    );
    fixture
        .blueprint
        .components
        .get_mut("central-brain")
        .unwrap()
        .conflicts_with
        .insert("review-notes".into());
    assert_eq!(fixture.plan().unwrap_err().code, "component_conflict");
    fixture.request.optional_components.insert("unknown".into());
    assert_eq!(fixture.plan().unwrap_err().code, "unknown_component");
}

#[test]
fn artifact_bytes_are_checked_and_paths_cannot_escape_packs() {
    let mut fixture = Fixture::new();
    fixture
        .artifacts
        .get_mut("central-brain")
        .unwrap()
        .push(b' ');
    assert_eq!(fixture.plan().unwrap_err().code, "artifact_digest_mismatch");
    fixture.artifacts.remove("central-brain");
    assert_eq!(fixture.plan().unwrap_err().code, "artifact_missing");
    for path in [
        "../outside",
        "/absolute",
        "C:\\host",
        "https://example.org/entry",
        "agents/../entry",
        "agents//entry",
    ] {
        fixture
            .blueprint
            .components
            .get_mut("central-brain")
            .unwrap()
            .artifact
            .path = path.into();
        assert_eq!(
            fixture.plan().unwrap_err().code,
            "invalid_artifact_path",
            "{path}"
        );
    }
}

#[test]
fn effective_policy_can_only_narrow_and_preferences_cannot_raise_authority() {
    let mut fixture = Fixture::new();
    fixture.policy.max_tokens_per_run = 1024;
    assert_eq!(fixture.plan().unwrap().constraints.max_tokens_per_run, 1024);
    fixture.policy.max_tokens_per_run = 100_000;
    assert_eq!(fixture.plan().unwrap().constraints.max_tokens_per_run, 4096);
    fixture
        .request
        .preferences
        .insert("review-required".into(), PreferenceValue::Boolean(false));
    assert_eq!(fixture.plan().unwrap_err().code, "override_denied");
    fixture.request.preferences.clear();
    fixture.request.preferences.insert(
        "max_tokens_per_run".into(),
        PreferenceValue::Integer(100_000),
    );
    assert_eq!(fixture.plan().unwrap_err().code, "unknown_preference");
    fixture.request.preferences.clear();
    fixture
        .approved_models
        .get_mut("local.fixture")
        .unwrap()
        .uses_network = true;
    assert_eq!(fixture.plan().unwrap_err().code, "provider_denied");
    fixture
        .approved_models
        .get_mut("local.fixture")
        .unwrap()
        .uses_network = false;
    fixture.policy.allowed_providers.clear();
    assert_eq!(fixture.plan().unwrap_err().code, "provider_denied");
}

#[test]
fn required_capability_dominates_optional_regardless_of_component_order() {
    let mut fixture = Fixture::new();
    fixture
        .blueprint
        .components
        .get_mut("central-brain")
        .unwrap()
        .optional_capabilities
        .insert("documents.read".into());
    fixture
        .blueprint
        .components
        .get_mut("review-notes")
        .unwrap()
        .required_capabilities
        .insert("documents.read".into());
    fixture
        .request
        .optional_components
        .insert("review-notes".into());
    assert_eq!(fixture.plan().unwrap_err().code, "capability_unbound");
    fixture.request.connectors.insert(
        "documents.read".into(),
        ConnectorBinding {
            connection_id: "account-a".into(),
            generation: "generation-a".into(),
        },
    );
    let plan = fixture.plan().unwrap();
    assert!(plan.required_capabilities.contains("documents.read"));
    assert!(!plan.optional_capabilities.contains("documents.read"));
    assert!(plan.unresolved_optional_capabilities.is_empty());
}

#[test]
fn missing_optional_binding_is_visible_but_not_blocking() {
    let mut fixture = Fixture::new();
    fixture
        .blueprint
        .components
        .get_mut("central-brain")
        .unwrap()
        .optional_capabilities
        .insert("documents.read".into());
    assert!(fixture
        .plan()
        .unwrap()
        .unresolved_optional_capabilities
        .contains("documents.read"));
    fixture.request.models.clear();
    assert_eq!(fixture.plan().unwrap_err().code, "model_unbound");
}

#[test]
fn lock_identity_covers_configuration_authority_bindings_and_versions() {
    let original = Fixture::new().hash();
    let mut fixture = Fixture::new();
    fixture.request.customer_config_revision = "b".repeat(64);
    assert_ne!(original, fixture.hash());
    let mut fixture = Fixture::new();
    fixture.context.org_units.push("finance".into());
    assert_ne!(original, fixture.hash());
    let mut fixture = Fixture::new();
    fixture.context.policy_version = Some(2);
    assert_ne!(original, fixture.hash());
    let mut fixture = Fixture::new();
    fixture
        .approved_models
        .get_mut("local.fixture")
        .unwrap()
        .model = "replacement-model".into();
    assert_ne!(original, fixture.hash());
    let mut fixture = Fixture::new();
    fixture.blueprint.solution.version = "0.1.1".into();
    assert_ne!(original, fixture.hash());
    let mut fixture = Fixture::new();
    fixture.context.assertion_id = "rotated-session".into();
    fixture.context.issued_at_ms = 1100;
    fixture.context.expires_at_ms = 2100;
    assert_eq!(original, fixture.hash());
}

#[test]
fn tenant_and_instance_namespaces_prevent_accidental_resource_adoption() {
    let mut fixture = Fixture::new();
    let original = fixture.plan().unwrap().components["central-brain"]
        .resource_id
        .clone();
    fixture.context.tenant_context.workspace_id = "workspace-b".into();
    assert_ne!(
        original,
        fixture.plan().unwrap().components["central-brain"].resource_id
    );
    fixture.context.tenant_context.workspace_id = "workspace-a".into();
    fixture.request.instance_id = "second-instance".into();
    assert_ne!(
        original,
        fixture.plan().unwrap().components["central-brain"].resource_id
    );
}

#[test]
fn expired_local_or_inconsistent_identity_never_falls_back_to_shared() {
    let mut fixture = Fixture::new();
    fixture.context.expires_at_ms = 1500;
    assert_eq!(
        fixture.plan().unwrap_err().code,
        "verified_identity_required"
    );
    let mut fixture = Fixture::new();
    fixture.context.tenant_context = TenantContext::local_implicit();
    assert_eq!(
        fixture.plan().unwrap_err().code,
        "verified_identity_required"
    );
    let mut fixture = Fixture::new();
    fixture.context.tenant_context.actor_id = Some("spoofed".into());
    assert_eq!(
        fixture.plan().unwrap_err().code,
        "verified_identity_required"
    );
}

#[test]
fn incompatible_engine_and_missing_host_readiness_fail_before_planning() {
    let fixture = Fixture::new();
    let mut input = ResolutionInput {
        host_facts_sha256: None,
        request: &fixture.request,
        approved_models: &fixture.approved_models,
        verified_context: &fixture.context,
        now_ms: 1500,
        engine_version: "0.6.0",
        deployment_policy: &fixture.policy,
        available_deployment_requirements: &fixture.blueprint.deployment_requirements,
        artifacts: &fixture.artifacts,
    };
    assert_eq!(
        resolve(&fixture.blueprint, input).unwrap_err().code,
        "incompatible_engine"
    );
    let empty = Default::default();
    input = ResolutionInput {
        host_facts_sha256: None,
        request: &fixture.request,
        approved_models: &fixture.approved_models,
        verified_context: &fixture.context,
        now_ms: 1500,
        engine_version: "0.7.2",
        deployment_policy: &fixture.policy,
        available_deployment_requirements: &empty,
        artifacts: &fixture.artifacts,
    };
    assert_eq!(
        resolve(&fixture.blueprint, input).unwrap_err().code,
        "deployment_requirement_missing"
    );
}

#[test]
fn unsupported_memory_stores_cannot_be_introduced_by_labels() {
    for store in ["team", "curated", "local_noop"] {
        assert_eq!(
            parse_blueprint(&FIXTURE.replace("\"private_user\"", &format!("\"{store}\"")))
                .unwrap_err()
                .code,
            "invalid_schema"
        );
    }
}

#[test]
fn customer_model_metadata_is_rejected() {
    let mut request = serde_json::to_value(Fixture::new().request).unwrap();
    request["models"]["economy"] = serde_json::json!({
        "provider": "remote",
        "model": "network-model",
        "credential_ref": "remote.account",
        "uses_network": false
    });
    assert!(serde_json::from_value::<InstallRequest>(request).is_err());
}

#[test]
fn model_references_require_current_host_approval() {
    let mut fixture = Fixture::new();
    let plan = fixture.plan().unwrap();
    assert_eq!(plan.models["economy"].binding_id, "local.fixture");
    assert_eq!(
        plan.models["economy"].binding,
        fixture.approved_models["local.fixture"]
    );
    fixture
        .request
        .models
        .insert("economy".into(), "unapproved".into());
    assert_eq!(fixture.plan().unwrap_err().code, "model_binding_unapproved");
    fixture
        .request
        .models
        .insert("economy".into(), "local.fixture".into());
    fixture.approved_models.clear();
    assert_eq!(fixture.plan().unwrap_err().code, "model_binding_unapproved");
}

#[test]
fn host_metadata_controls_network_and_provider_policy() {
    let mut fixture = Fixture::new();
    // Even a reference named "local" must use the actual host metadata.
    let binding = fixture.approved_models.get_mut("local.fixture").unwrap();
    binding.provider = "remote".into();
    binding.uses_network = true;
    fixture
        .blueprint
        .constraints
        .allowed_providers
        .insert("remote".into());
    fixture.policy.allowed_providers.insert("remote".into());
    assert_eq!(fixture.plan().unwrap_err().code, "provider_denied");
    fixture.blueprint.constraints.allow_network_egress = true;
    assert_eq!(fixture.plan().unwrap_err().code, "provider_denied");
    fixture.policy.allow_network_egress = true;
    assert!(
        fixture.plan().unwrap().models["economy"]
            .binding
            .uses_network
    );
    fixture.blueprint.constraints.allow_network_egress = false;
    assert_eq!(fixture.plan().unwrap_err().code, "provider_denied");
    fixture.blueprint.constraints.allow_network_egress = true;
    fixture.policy.allowed_providers.remove("remote");
    assert_eq!(fixture.plan().unwrap_err().code, "provider_denied");
}

#[test]
fn lock_tracks_selected_binding_id_but_not_unused_host_bindings() {
    let mut fixture = Fixture::new();
    let before = fixture.hash();
    fixture.approved_models.insert(
        "replacement".into(),
        fixture.approved_models["local.fixture"].clone(),
    );
    assert_eq!(before, fixture.hash());
    fixture
        .request
        .models
        .insert("economy".into(), "replacement".into());
    assert_ne!(before, fixture.hash());
    assert_eq!(fixture.plan().unwrap().models.len(), 1);
}

#[test]
fn generated_schema_matches_checked_in_contract() {
    let checked_in: serde_json::Value = serde_json::from_str(include_str!(
        "../../../specs/solutions/solution-blueprint.schema.json"
    ))
    .unwrap();
    assert_eq!(
        serde_json::to_value(blueprint_schema()).unwrap(),
        checked_in
    );
}

#[test]
fn reviewed_composition_binds_host_fact_revision_and_preserves_legacy_locks() {
    let fixture = Fixture::new();
    let old = fixture.plan().unwrap();
    let bytes = serde_json::to_vec(&old).unwrap();
    assert!(!String::from_utf8(bytes.clone())
        .unwrap()
        .contains("host_facts_sha256"));
    let roundtrip: ResolvedPlan = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        old.composition_hash().unwrap(),
        roundtrip.composition_hash().unwrap()
    );
    let first = sha256(b"source binding revision 1; provider endpoint A");
    let second = sha256(b"source binding revision 2; provider endpoint B");
    let a = fixture.plan_with_host_facts(Some(&first)).unwrap();
    let b = fixture.plan_with_host_facts(Some(&second)).unwrap();
    assert_eq!(a.host_facts_sha256.as_deref(), Some(first.as_str()));
    assert_ne!(
        old.composition_hash().unwrap(),
        a.composition_hash().unwrap()
    );
    assert_ne!(a.composition_hash().unwrap(), b.composition_hash().unwrap());
    for invalid in ["", "endpoint-secret", "123", &"z".repeat(64)] {
        assert!(fixture.plan_with_host_facts(Some(invalid)).is_err());
    }
}
