// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::{BTreeMap, BTreeSet};
use tandem_enterprise_contract::{
    AuthorityChain, HumanActor, RequestPrincipal, TenantContext, TenantContextAssertionClaims,
    VerifiedTenantContext,
};
use tandem_solutions::*;

const A: &str = include_str!("../fixtures/company-brain-text/customer-a.yaml");
const B: &str = include_str!("../fixtures/company-brain-text/customer-b.yaml");

struct Fixture {
    blueprint: SolutionBlueprint,
    config: CustomerConfig,
    context: VerifiedTenantContext,
    refs: BTreeSet<String>,
    subjects: BTreeSet<String>,
    projects: BTreeSet<String>,
    units: BTreeSet<String>,
    connectors: BTreeMap<String, ConnectorBinding>,
}

impl Fixture {
    fn new(document: &str, customer: &str) -> Self {
        let actor = format!("owner-{customer}");
        Self {
            blueprint: parse_blueprint(include_str!(
                "../fixtures/company-brain-text/solution.json"
            ))
            .unwrap(),
            config: parse_customer_config(document).unwrap(),
            context: TenantContextAssertionClaims::new_v1(
                "issuer",
                "runtime",
                1000,
                2000,
                "synthetic-assertion",
                TenantContext::explicit_user_workspace(
                    format!("org-{customer}"),
                    format!("workspace-{customer}"),
                    Some(format!("deployment-{customer}")),
                    &actor,
                ),
                HumanActor::tandem_user(&actor),
                AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                    &actor, "fixture",
                )),
                vec!["workspace:user".into()],
            )
            .into(),
            refs: [
                format!("profile-ref:synthetic-company-{customer}"),
                format!("data-ref:synthetic-notes-{customer}"),
            ]
            .into(),
            subjects: [actor].into(),
            projects: [format!("project-{customer}")].into(),
            units: BTreeSet::new(),
            connectors: BTreeMap::new(),
        }
    }
    fn prepare(
        &self,
        current: Option<&str>,
        expected: Option<&str>,
    ) -> Result<PreparedCustomerConfig, SolutionError> {
        prepare_customer_config(
            &self.blueprint,
            &self.config,
            CustomerConfigInput {
                verified_context: &self.context,
                selected_scope: &self.config.scope,
                now_ms: 1500,
                current_revision: current,
                expected_revision: expected,
                host_policy: &self.blueprint.constraints,
                approved_references: &self.refs,
                approved_subjects: &self.subjects,
                approved_org_units: &self.units,
                approved_projects: &self.projects,
                approved_connectors: &self.connectors,
            },
        )
    }
    fn plan(&self) -> ResolvedPlan {
        let prepared = self.prepare(None, None).unwrap();
        let models = serde_json::from_str(include_str!(
            "../fixtures/company-brain-text/host-models.json"
        ))
        .unwrap();
        let artifacts = BTreeMap::from([(
            "central-brain".into(),
            include_bytes!("../fixtures/company-brain-text/agents/central-brain.json").to_vec(),
        )]);
        resolve(
            &self.blueprint,
            ResolutionInput {
                host_facts_sha256: None,
                request: &prepared.request,
                verified_context: &self.context,
                now_ms: 1500,
                engine_version: "0.7.2",
                deployment_policy: &prepared.deployment_policy,
                available_deployment_requirements: &self.blueprint.deployment_requirements,
                approved_models: &models,
                artifacts: &artifacts,
            },
        )
        .unwrap()
    }
}

#[test]
fn two_customers_resolve_one_artifact_with_distinct_scopes_and_bounded_overrides() {
    let a = Fixture::new(A, "a");
    let b = Fixture::new(B, "b");
    let (pa, pb) = (a.plan(), b.plan());
    assert_eq!(pa.blueprint_sha256, pb.blueprint_sha256);
    assert_eq!(
        pa.components["central-brain"].artifact,
        pb.components["central-brain"].artifact
    );
    assert_ne!(
        pa.components["central-brain"].resource_id,
        pb.components["central-brain"].resource_id
    );
    assert_ne!(pa.customer_config_revision, pb.customer_config_revision);
    assert_eq!(pa.constraints.max_tokens_per_run, 2048);
    assert_eq!(pb.constraints.max_tokens_per_run, 1024);
    assert!(!pa.constraints.allow_network_egress);
    assert!(pa.connectors.is_empty());
    assert!(pb.connectors.is_empty());
    assert_eq!(
        pb.preferences["reply-language"],
        PreferenceValue::Choice("fr".into())
    );
}

#[test]
fn parser_rejects_credentials_duplicate_keys_authority_claims_and_unsupported_stores() {
    for input in [
        format!("{A}\nroles: [admin]\n"),
        format!("{A}\nschema_version: '1'\n"),
        A.replace(
            "secret_refs: {}",
            "secret_refs: {mail: 'raw-password-value'}",
        ),
        A.replace("kind: private_user", "kind: team"),
        A.replace(
            "profile-ref:synthetic-company-a",
            "https://user:password@example.invalid",
        ),
    ] {
        assert!(parse_customer_config(&input).is_err());
    }
}

#[test]
fn current_identity_selection_approved_references_and_revisions_are_required() {
    let mut f = Fixture::new(A, "a");
    let revision = customer_config_revision(&f.config).unwrap();
    assert!(f.prepare(Some(&revision), Some(&revision)).is_ok());
    assert_eq!(
        f.prepare(Some(&revision), None).err().unwrap().code,
        "customer_revision_conflict"
    );
    f.config.scope.org_id = "org-b".into();
    assert_eq!(
        f.prepare(None, None).err().unwrap().code,
        "customer_scope_mismatch"
    );
    f.config.scope.org_id = "org-a".into();
    f.config
        .data_refs
        .insert("notes".into(), "data-ref:synthetic-notes-b".into());
    assert_eq!(
        f.prepare(None, None).err().unwrap().code,
        "customer_reference_denied"
    );
    f.config.data_refs.clear();
    f.config.memory_spaces.insert(
        "private".into(),
        CustomerMemorySpace::PrivateUser {
            subject_id: "owner-b".into(),
        },
    );
    assert_eq!(
        f.prepare(None, None).err().unwrap().code,
        "customer_memory_binding_denied"
    );
    f.config.memory_spaces.remove("private");
    assert_eq!(
        f.prepare(None, None).err().unwrap().code,
        "customer_memory_binding_denied"
    );
}

#[test]
fn sanitized_template_contains_no_customer_values_and_cannot_be_imported_as_deployable_config() {
    let a = Fixture::new(A, "a");
    let b = Fixture::new(B, "b");
    let exported = canonical_json(&customer_config_template(&a.blueprint).unwrap()).unwrap();
    assert_eq!(
        exported,
        canonical_json(&customer_config_template(&b.blueprint).unwrap()).unwrap()
    );
    let text = String::from_utf8(exported).unwrap();
    for value in [
        "org-a",
        "workspace-a",
        "owner-a",
        "synthetic-company",
        "synthetic-notes",
        "Europe/Budapest",
        "local.fixture",
    ] {
        assert!(!text.contains(value));
    }
    assert!(parse_customer_config(&text).is_err());
    // Customer-owned recovery serialization preserves references, never values.
    let backup = String::from_utf8(canonical_json(&a.config).unwrap()).unwrap();
    assert_eq!(parse_customer_config(&backup).unwrap(), a.config);
}

#[test]
fn customer_constraints_cannot_raise_host_ceilings_or_enable_egress() {
    let mut f = Fixture::new(A, "a");
    f.config.constraints.allow_network_egress = true;
    f.config.constraints.max_tokens_per_run = u64::MAX;
    f.config
        .constraints
        .allowed_providers
        .insert("external".into());
    let plan = f.plan();
    assert!(!plan.constraints.allow_network_egress);
    assert_eq!(plan.constraints.max_tokens_per_run, 4096);
    assert_eq!(plan.constraints.allowed_providers, ["local".into()].into());
}

#[test]
fn selected_install_expiry_and_connector_generation_cannot_come_from_config() {
    let mut f = Fixture::new(A, "a");
    let selected = f.config.scope.clone();
    f.config.scope.instance_id = "another-install".into();
    let error = prepare_customer_config(
        &f.blueprint,
        &f.config,
        CustomerConfigInput {
            verified_context: &f.context,
            selected_scope: &selected,
            now_ms: 1500,
            current_revision: None,
            expected_revision: None,
            host_policy: &f.blueprint.constraints,
            approved_references: &f.refs,
            approved_subjects: &f.subjects,
            approved_org_units: &f.units,
            approved_projects: &f.projects,
            approved_connectors: &f.connectors,
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "customer_scope_mismatch");
    f.config.scope = selected;
    f.config.connectors.insert(
        "mail.read".into(),
        ConnectorBinding {
            connection_id: "another-account".into(),
            generation: "stale".into(),
        },
    );
    assert_eq!(
        f.prepare(None, None).err().unwrap().code,
        "customer_connector_denied"
    );
    f.config.connectors.clear();
    f.context.expires_at_ms = 1400;
    assert_eq!(
        f.prepare(None, None).err().unwrap().code,
        "verified_identity_required"
    );
}

#[test]
fn review_diff_separates_upstream_changes_without_printing_customer_identifiers() {
    let a = Fixture::new(A, "a");
    let mut b = Fixture::new(B, "b");
    let changes =
        customer_config_changes(&a.blueprint, &b.blueprint, &a.config, &b.config).unwrap();
    assert!(!changes.upstream_changed);
    assert!(changes.customer_fields.contains("scope"));
    assert!(changes.customer_fields.contains("preferences"));
    assert!(!String::from_utf8(canonical_json(&changes).unwrap())
        .unwrap()
        .contains("org-a"));
    b.blueprint.solution.version = "0.1.1".into();
    assert!(
        customer_config_changes(&a.blueprint, &b.blueprint, &a.config, &a.config)
            .unwrap()
            .upstream_changed
    );
}
