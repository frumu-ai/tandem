use super::solution_tests::{fixture, request, signed};
use super::tests::EnvGuard;
use super::*;
use crate::solution_installation::{SolutionConfigurationRequest, SolutionStagingRequest};
use crate::stateful_runtime::orchestration_store::{
    OrchestrationStateStore, SolutionComponentProgress,
};
use crate::AppState;
use tandem_enterprise_contract::{
    hosted_policy::hosted_unit_principal, AccessPermission, AuthorityChain, ConnectorInstance,
    DataClass, HumanActor, OrganizationUnitAccessGrant, PrincipalRef, RequestPrincipal,
    ResourceKind, ResourceRef, SourceBinding, TenantContext, TenantContextAssertionClaims,
    VerifiedTenantContext,
};

struct Fixture {
    root: tempfile::TempDir,
    state: AppState,
    verified: VerifiedTenantContext,
    configuration: SolutionConfigurationRequest,
    _keys: EnvGuard,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let archive = root.path().join("solution.zip");
        let key = signed(&archive, &fixture());
        let keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &key);
        let mut state = crate::test_support::test_state().await;
        state.automation_v2_runs_path = root.path().join("runtime/runs.json");
        state.routines_path = root.path().join("routines.json");
        state.routine_runs_path = root.path().join("routine-runs.json");
        state.enterprise.hosted_policy_revision_path =
            root.path().join("hosted-policy-revision.json");
        state.pack_manager = Arc::new(PackManager::new(root.path().join("packs")));
        state.pack_manager.install(request(&archive)).await.unwrap();
        let settings = serde_json::json!({"solution_installation": {
            "schema_version": 1,
            "policy": {"allowed_providers": ["local"], "allow_network_egress": false,
                "max_tokens_per_run": 1024, "max_concurrent_runs": 1, "max_daily_cost_microusd": 0},
            "models": {"local.fixture": {"provider_id": "local", "model_id": "echo-1",
                "credential_ref": "secret-ref:local-fixture", "allow_test_provider": true}}
        }});
        let mut runtime = state.runtime.wait().clone();
        runtime.config =
            tandem_core::ConfigStore::new(root.path().join("config.json"), Some(settings))
                .await
                .unwrap();
        runtime.providers = tandem_providers::ProviderRegistry::new(Default::default());
        runtime.workspace_index = tandem_runtime::WorkspaceIndex::new(root.path()).await;
        state.runtime = Arc::new(std::sync::OnceLock::from(runtime));
        let tenant =
            TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice");
        let now = crate::now_ms();
        let mut claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            now,
            now + 300_000,
            "assertion-a",
            tenant.clone(),
            HumanActor::tandem_user("alice"),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                "alice",
                "tandem-web",
            )),
            vec!["hosted:role:admin".into()],
        );
        claims.policy_version = Some(1);
        claims.capabilities = vec!["hosted.use".into(), "hosted.admin".into()];
        claims.org_units = vec!["eng".into()];
        let mut resource = ResourceRef::new("org-a", "dep-a", ResourceKind::Document, "notes");
        resource.project_id = Some("project-a".into());
        state.enterprise.connectors.write().await.insert(
            "connector-a".into(),
            ConnectorInstance::active(
                "connector-a",
                tenant.clone(),
                "synthetic",
                PrincipalRef::human_user("alice"),
                now,
            ),
        );
        state.enterprise.source_bindings.write().await.insert(
            "notes".into(),
            SourceBinding::enabled(
                "notes",
                tenant.clone(),
                "connector-a",
                "synthetic",
                "notes",
                resource.clone(),
                DataClass::Internal,
                PrincipalRef::human_user("alice"),
                now,
            ),
        );
        state
            .enterprise
            .org_unit_access_grants
            .write()
            .await
            .insert(
                "read-notes".into(),
                OrganizationUnitAccessGrant::active(
                    "read-notes",
                    tenant,
                    hosted_unit_principal("eng"),
                    resource,
                    now,
                )
                .with_permissions(vec![AccessPermission::Read])
                .with_data_classes(vec![DataClass::Internal]),
            );
        let mut config = tandem_solutions::parse_customer_config(include_str!(
            "../../tandem-solutions/fixtures/company-brain-text/customer-a.yaml"
        ))
        .unwrap();
        config.scope.workspace_id = "dep-a".into();
        config.scope.deployment_id = "dep-a".into();
        config.profile_ref = "profile-ref:org-a".into();
        config
            .data_refs
            .insert("notes".into(), "data-ref:notes".into());
        config.memory_spaces.insert(
            "private".into(),
            tandem_solutions::CustomerMemorySpace::PrivateUser {
                subject_id: "alice".into(),
            },
        );
        config.optional_components.insert("review-notes".into());
        let this = Self {
            root,
            state,
            verified: claims.into(),
            configuration: SolutionConfigurationRequest {
                pack_selector: "tandem.company-brain".into(),
                configuration: config,
            },
            _keys: keys,
        };
        this.policy(1, true).await;
        this
    }

    async fn policy(&self, version: u64, active: bool) {
        let now = crate::now_ms();
        let path = self.root.path().join("policy.json");
        let raw = serde_json::to_vec(&serde_json::json!({
            "schema_version": 1, "policy_version": version, "organization_id": "org-a", "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": [{"id": "alice", "email": null, "username": null, "role": "admin",
                "capabilities": ["hosted.use", "hosted.admin"], "is_active": active, "email_verified": true}],
            "org_units": [{"id": "eng", "slug": "eng", "display_name": "Engineering", "kind": "department", "state": "active"}],
            "org_unit_memberships": [{"unit_id": "eng", "user_id": "alice"}], "deployment_grants": []
        })).unwrap();
        std::fs::write(&path, raw).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        self.state
            .enterprise
            .hosted_policy
            .configure_test_source("org-a", "dep-a", path);
        self.state.reload_hosted_policy().await.unwrap();
    }

    async fn review_and_save(&self) -> SolutionStagingRequest {
        let preview = self
            .state
            .preview_solution_configuration(&self.verified, &self.configuration)
            .await
            .unwrap();
        assert!(!preview.solution_ready);
        assert!(preview.plan.host_facts_sha256.is_some());
        assert_eq!(preview.plan.constraints.max_tokens_per_run, 1024);
        let stored = self
            .state
            .save_solution_configuration(&self.verified, self.configuration.clone(), None)
            .await
            .unwrap();
        SolutionStagingRequest {
            pack_selector: self.configuration.pack_selector.clone(),
            scope: stored.config.scope,
            config_version: stored.version,
            reviewed_composition: preview.composition_sha256,
            expected_generation: None,
        }
    }
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_service_signed_pack_preview_save_stage_restart_and_drift() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            let fixture = Fixture::new().await;
            let paths = crate::OrchestrationStorePaths::from_automation_runs_path(
                &fixture.state.automation_v2_runs_path,
            );
            assert!(!paths.database_path.exists());
            fixture
                .state
                .preview_solution_configuration(&fixture.verified, &fixture.configuration)
                .await
                .unwrap();
            assert!(
                !paths.database_path.exists(),
                "preview must not create a state database"
            );
            let request = fixture.review_and_save().await;
            let staged = fixture
                .state
                .stage_solution_installation(&fixture.verified, request.clone())
                .await
                .unwrap();
            assert!(staged.all_components_staged());
            assert_eq!(staged.generation, 5);
            let mut restarted = fixture.state.clone();
            restarted.agent_teams = crate::agent_teams::AgentTeamRuntime::new(
                fixture.root.path().join("restart-audit.jsonl"),
            );
            restarted.load_routines().await.unwrap();
            assert_eq!(
                restarted
                    .stage_solution_installation(&fixture.verified, request.clone())
                    .await
                    .unwrap(),
                staged
            );
            let agent = &staged.plan.components["central-brain"].resource_id;
            let workspace = fixture.state.workspace_index.snapshot().await.root;
            let path = Path::new(&workspace)
                .join(".tandem/agent-team/templates")
                .join(format!("{agent}.yaml"));
            let observed: tandem_orchestrator::AgentTemplate =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert!(!observed.enabled);
            assert_eq!(observed.default_budget.max_tokens, Some(1024));
            assert_eq!(observed.default_model.unwrap()["model_id"], "echo-1");
            std::fs::remove_file(&path).unwrap();
            assert!(restarted
                .stage_solution_installation(&fixture.verified, request)
                .await
                .is_err());
            assert!(
                !path.exists(),
                "staged receipt validation must not recreate a deleted resource"
            );
            let store = OrchestrationStateStore::from_automation_runs_path(
                &fixture.state.automation_v2_runs_path,
            )
            .unwrap();
            assert_eq!(
                store
                    .solution_installation(
                        &fixture.verified,
                        &fixture.configuration.configuration.scope,
                        crate::now_ms()
                    )
                    .unwrap()
                    .unwrap(),
                staged
            );
        },
    )
    .await;
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_service_rechecks_native_grants_sources_scope_and_revocation() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            let fixture = Fixture::new().await;
            let request = fixture.review_and_save().await;
            fixture
                .state
                .enterprise
                .source_bindings
                .write()
                .await
                .get_mut("notes")
                .unwrap()
                .updated_at_ms += 1;
            assert!(
                fixture
                    .state
                    .stage_solution_installation(&fixture.verified, request.clone())
                    .await
                    .is_err(),
                "same source ID with changed revision must invalidate preview"
            );
            fixture
                .state
                .enterprise
                .source_bindings
                .write()
                .await
                .get_mut("notes")
                .unwrap()
                .updated_at_ms -= 1;
            let mut other = fixture.configuration.clone();
            other.configuration.scope.org_id = "org-b".into();
            assert!(fixture
                .state
                .preview_solution_configuration(&fixture.verified, &other)
                .await
                .is_err());
            other = fixture.configuration.clone();
            other.configuration.memory_spaces.insert(
                "private".into(),
                tandem_solutions::CustomerMemorySpace::PrivateUser {
                    subject_id: "bob".into(),
                },
            );
            assert!(fixture
                .state
                .preview_solution_configuration(&fixture.verified, &other)
                .await
                .is_err());
            let grant = fixture
                .state
                .enterprise
                .org_unit_access_grants
                .write()
                .await
                .remove("read-notes")
                .unwrap();
            assert!(
                fixture
                    .state
                    .preview_solution_configuration(&fixture.verified, &fixture.configuration)
                    .await
                    .is_err(),
                "hosted admin does not imply document read access"
            );
            fixture
                .state
                .enterprise
                .org_unit_access_grants
                .write()
                .await
                .insert("read-notes".into(), grant);
            fixture.policy(2, false).await;
            assert!(fixture
                .state
                .stage_solution_installation(&fixture.verified, request)
                .await
                .is_err());
        },
    )
    .await;
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_service_uses_operator_policy_and_resumes_partial_native_failure() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            let fixture = Fixture::new().await;
            fixture
                .state
                .config
                .patch_runtime(serde_json::json!({"solution_installation": {
                    "policy": {"max_tokens_per_run": 999999, "allow_network_egress": true}
                }}))
                .await
                .unwrap();
            let request = fixture.review_and_save().await;
            // An existing malformed native file blocks the second component after
            // the first has a durable receipt. Recovery requires resolving that
            // concrete conflict; the service cannot overwrite it.
            std::fs::write(&fixture.state.routines_path, b"manual conflict").unwrap();
            assert!(fixture
                .state
                .stage_solution_installation(&fixture.verified, request.clone())
                .await
                .is_err());
            assert_eq!(
                std::fs::read(&fixture.state.routines_path).unwrap(),
                b"manual conflict"
            );
            let store = OrchestrationStateStore::from_automation_runs_path(
                &fixture.state.automation_v2_runs_path,
            )
            .unwrap();
            let interrupted = store
                .solution_installation(&fixture.verified, &request.scope, crate::now_ms())
                .unwrap()
                .unwrap();
            assert!(matches!(
                interrupted.components["central-brain"],
                SolutionComponentProgress::Staged { .. }
            ));
            assert!(matches!(
                interrupted.components["review-notes"],
                SolutionComponentProgress::Claimed { .. }
            ));
            std::fs::remove_file(&fixture.state.routines_path).unwrap();
            let (first, second) = tokio::join!(
                fixture
                    .state
                    .stage_solution_installation(&fixture.verified, request.clone()),
                fixture
                    .state
                    .stage_solution_installation(&fixture.verified, request.clone())
            );
            assert!(first.is_ok() || second.is_ok());
            let resumed = fixture
                .state
                .stage_solution_installation(&fixture.verified, request)
                .await
                .unwrap();
            assert!(resumed.all_components_staged());
            assert_eq!(
                resumed.components["central-brain"],
                interrupted.components["central-brain"]
            );
            assert_eq!(resumed.components.len(), 2);
        },
    )
    .await;
}
