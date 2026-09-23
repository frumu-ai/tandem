use super::*;
use tandem_providers::{ProviderCredentialKind, ProviderRegistry};

const BINDING: &str = "local.fixture";
const REFERENCE: &str = "secret-ref:model-service";
const TOKEN: &str = "synthetic-model-account-fixture";

async fn stored_key(fixture: &Fixture, token: &str) -> String {
    let dir = crate::http::config_providers::provider_auth_security_dir_for_state(&fixture.state);
    let token = token.to_string();
    tokio::task::spawn_blocking(move || {
        let tenant = TenantContext::local_implicit();
        tandem_providers::set_provider_auth_for_tenant_in_dir(&dir, &tenant, "llama_cpp", &token)
            .unwrap();
        tandem_providers::provider_credential_revision_for_tenant_in_dir(
            &dir,
            &tenant,
            ProviderCredentialKind::ApiKey,
            "llama_cpp",
        )
        .unwrap()
        .authorization_revision
    })
    .await
    .unwrap()
}

async fn configure(fixture: &mut Fixture, revision: &str, token: &str) {
    configure_at(fixture, revision, token, "http://127.0.0.1:9/v1").await;
}

async fn configure_at(fixture: &mut Fixture, revision: &str, token: &str, url: &str) {
    let mut runtime = fixture.state.runtime.wait().clone();
    runtime.config = tandem_core::ConfigStore::new(fixture.root.path().join("model-config.json"),
        Some(serde_json::json!({"solution_installation": {
            "schema_version": 1, "policy": {"allowed_providers": ["llama_cpp"],
                "allow_network_egress": true, "max_tokens_per_run": 1024,
                "max_concurrent_runs": 1, "max_daily_cost_microusd": 100},
            "models": {BINDING: {"provider_id": "llama_cpp", "model_id": "synthetic-model",
                "credential_ref": REFERENCE, "account": {"credential_kind": "api_key",
                    "credential_location": "host_service", "authorization_revision": revision,
                    "resource": ResourceRef::new("org-a", "dep-a", ResourceKind::SecretProviderCredential, REFERENCE)}}}
        }}))).await.unwrap();
    runtime.providers = ProviderRegistry::new(
        serde_json::from_value(serde_json::json!({
            "default_provider": "llama_cpp", "providers": {"llama_cpp": {
                "url": url, "api_key": token, "default_model": "synthetic-model"}}
        }))
        .unwrap(),
    );
    fixture.state.runtime = Arc::new(std::sync::OnceLock::from(runtime));
}

async fn set_route_review(fixture: &mut Fixture, revision: &str, expires_at_ms: u64) {
    let endpoint_sha256 = fixture
        .state
        .providers
        .runtime_binding_for_tenant(
            &fixture.verified.tenant_context,
            "llama_cpp",
            "synthetic-model",
        )
        .await
        .unwrap()
        .endpoint_sha256;
    let mut cli = fixture.state.config.get_layers_value().await["cli"].clone();
    cli["solution_installation"]["models"][BINDING]["review"] = serde_json::json!({
        "schema_version": 1,
        "revision": "operator-reviewed-route-v1",
        "provider_id": "llama_cpp",
        "model_id": "synthetic-model",
        "credential_ref": REFERENCE,
        "authorization_revision": revision,
        "endpoint_sha256": endpoint_sha256,
        "reviewed_until_ms": expires_at_ms,
        "modalities": ["text"],
        "supports_tool_use": false,
        "processing_regions": ["local"],
        "retention_hours": 1,
        "price": {
            "input_microusd_per_million": 1000000,
            "output_microusd_per_million": 1000000,
            "request_microusd": 1,
            "valid_until_ms": expires_at_ms
        }
    });
    let mut runtime = fixture.state.runtime.wait().clone();
    runtime.config = tandem_core::ConfigStore::new(
        fixture.root.path().join("reviewed-model-config.json"),
        Some(cli),
    )
    .await
    .unwrap();
    fixture.state.runtime = Arc::new(std::sync::OnceLock::from(runtime));
}

async fn clear_route_review(fixture: &mut Fixture) {
    let mut cli = fixture.state.config.get_layers_value().await["cli"].clone();
    cli["solution_installation"]["models"][BINDING]
        .as_object_mut()
        .unwrap()
        .remove("review");
    let mut runtime = fixture.state.runtime.wait().clone();
    runtime.config = tandem_core::ConfigStore::new(
        fixture.root.path().join("unreviewed-model-config.json"),
        Some(cli),
    )
    .await
    .unwrap();
    fixture.state.runtime = Arc::new(std::sync::OnceLock::from(runtime));
}

async fn install_network_profile_pack(fixture: &mut Fixture) -> EnvGuard {
    let mut entries = fixture_with_profile();
    let (_, source) = entries
        .iter_mut()
        .find(|(path, _)| path == "solution.json")
        .unwrap();
    let mut blueprint: serde_json::Value = serde_json::from_str(source).unwrap();
    blueprint["constraints"]["allowed_providers"] = serde_json::json!(["llama_cpp"]);
    blueprint["constraints"]["allow_network_egress"] = true.into();
    *source = serde_json::to_string(&blueprint).unwrap();
    let archive = fixture.root.path().join("reviewed-model-solution.zip");
    let key = signed(&archive, &entries);
    let keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &key);
    fixture.state.pack_manager =
        Arc::new(PackManager::new(fixture.root.path().join("reviewed-packs")));
    fixture
        .state
        .pack_manager
        .install(request(&archive))
        .await
        .unwrap();
    fixture
        .configuration
        .configuration
        .constraints
        .allowed_providers = std::collections::BTreeSet::from(["llama_cpp".into()]);
    fixture
        .configuration
        .configuration
        .constraints
        .allow_network_egress = true;
    keys
}

async fn observe_private_route(
    fixture: &Fixture,
    verified: &VerifiedTenantContext,
    generation: u64,
    composition: &str,
) -> anyhow::Result<tandem_solutions::ModelRouteFacts> {
    fixture
        .state
        .providers
        .scope_tenant_provider_auth_with_recovery(
            verified.tenant_context.clone(),
            tandem_providers::ProviderAuthRecovery::new(|_| async { Ok(false) }),
            true,
            fixture.state.observe_current_model_route(
                verified,
                &fixture.configuration.configuration.scope,
                generation,
                composition,
                "economy",
            ),
        )
        .await
}

async fn grant(fixture: &Fixture, id: &str, unit: &str) {
    fixture
        .state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert(
            id.into(),
            OrganizationUnitAccessGrant::active(
                id,
                fixture.verified.tenant_context.clone(),
                hosted_unit_principal(unit),
                ResourceRef::new(
                    "org-a",
                    "dep-a",
                    ResourceKind::SecretProviderCredential,
                    REFERENCE,
                ),
                crate::now_ms(),
            )
            .with_permissions(vec![AccessPermission::Execute])
            .with_data_classes(vec![DataClass::Credential]),
        );
}

fn identity(actor: &str, version: u64) -> VerifiedTenantContext {
    let now = crate::now_ms();
    let mut claims = TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 300000,
        format!("model-{actor}-{version}"),
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), actor),
        HumanActor::tandem_user(actor),
        AuthorityChain::from_request(RequestPrincipal::authenticated_user(actor, "tandem-web")),
        vec!["hosted:role:member".into()],
    );
    claims.policy_version = Some(version);
    claims.capabilities = vec!["hosted.use".into()];
    claims.org_units = vec![if actor == "alice" { "eng" } else { "ops" }.into()];
    claims.into()
}

async fn two_user_policy(fixture: &Fixture, version: u64, bob_active: bool) {
    let now = crate::now_ms();
    let path = fixture.root.path().join("model-policy.json");
    let raw = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1, "policy_version": version, "organization_id": "org-a", "deployment_id": "dep-a",
        "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
        "users": [{"id": "alice", "role": "member", "capabilities": ["hosted.use"], "is_active": true, "email_verified": true},
            {"id": "bob", "role": "member", "capabilities": ["hosted.use"], "is_active": bob_active, "email_verified": true}],
        "org_units": [{"id": "eng", "slug": "eng", "display_name": "Engineering", "kind": "department", "state": "active"},
            {"id": "ops", "slug": "ops", "display_name": "Operations", "kind": "department", "state": "active"}],
        "org_unit_memberships": [{"unit_id": "eng", "user_id": "alice"}, {"unit_id": "ops", "user_id": "bob"}],
        "deployment_grants": []
    })).unwrap();
    std::fs::write(&path, raw).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    fixture
        .state
        .enterprise
        .hosted_policy
        .configure_test_source("org-a", "dep-a", path);
    fixture.state.reload_hosted_policy().await.unwrap();
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_service_model_account_checks_current_user_grants_and_revocation() {
    let mut f = Fixture::new().await;
    f.state.memory_db_path = f.root.path().join("memory.sqlite");
    let revision = stored_key(&f, TOKEN).await;
    configure(&mut f, &revision, TOKEN).await;
    two_user_policy(&f, 2, true).await;
    let alice = identity("alice", 2);
    let bob = identity("bob", 2);
    let scope = &f.configuration.configuration.scope;
    // Hosted use alone, even an installer's earlier admin identity, is not a
    // credential resource grant. Both actors start denied.
    assert!(f
        .state
        .authorize_solution_model_account(&alice, scope, BINDING)
        .await
        .is_err());
    grant(&f, "model-eng", "eng").await;
    let first = f
        .state
        .authorize_solution_model_account(&alice, scope, BINDING)
        .await
        .unwrap();
    assert_eq!(first.verified.human_actor.actor_id, "alice");
    assert_eq!(first.binding.revision.authorization_revision, revision);
    assert!(f
        .state
        .authorize_solution_model_account(&bob, scope, BINDING)
        .await
        .is_err());
    grant(&f, "model-ops", "ops").await;
    let shared = f
        .state
        .authorize_solution_model_account(&bob, scope, BINDING)
        .await
        .unwrap();
    assert_eq!(shared.verified.human_actor.actor_id, "bob");
    assert_ne!(shared.authority_sha256, first.authority_sha256);
    f.state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .remove("model-ops");
    assert!(f
        .state
        .authorize_solution_model_account(&bob, scope, BINDING)
        .await
        .is_err());
    grant(&f, "model-ops", "ops").await;
    two_user_policy(&f, 3, false).await;
    assert!(f
        .state
        .authorize_solution_model_account(&identity("bob", 3), scope, BINDING)
        .await
        .is_err());
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_service_model_account_reconnect_requires_a_new_operator_revision() {
    let mut f = Fixture::new().await;
    f.state.memory_db_path = f.root.path().join("memory.sqlite");
    let original = stored_key(&f, TOKEN).await;
    configure(&mut f, &original, TOKEN).await;
    grant(&f, "model-eng", "eng").await;
    assert!(f
        .state
        .authorize_solution_model_account(
            &f.verified,
            &f.configuration.configuration.scope,
            BINDING
        )
        .await
        .is_ok());
    // Reconnecting the same material still creates a new authorization revision.
    let reconnected = stored_key(&f, TOKEN).await;
    assert_ne!(reconnected, original);
    assert!(f
        .state
        .authorize_solution_model_account(
            &f.verified,
            &f.configuration.configuration.scope,
            BINDING
        )
        .await
        .unwrap_err()
        .to_string()
        .contains("reconnected"));
    configure(&mut f, &reconnected, TOKEN).await;
    assert!(f
        .state
        .authorize_solution_model_account(
            &f.verified,
            &f.configuration.configuration.scope,
            BINDING
        )
        .await
        .is_ok());
    stored_key(&f, "different-synthetic-model-material").await;
    assert!(f
        .state
        .authorize_solution_model_account(
            &f.verified,
            &f.configuration.configuration.scope,
            BINDING
        )
        .await
        .is_err());
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_service_model_account_revokes_while_credential_lookup_waits() {
    let mut f = Fixture::new().await;
    f.state.memory_db_path = f.root.path().join("memory.sqlite");
    let revision = stored_key(&f, TOKEN).await;
    configure(&mut f, &revision, TOKEN).await;
    grant(&f, "model-eng", "eng").await;
    let directory = crate::http::config_providers::provider_auth_security_dir_for_state(&f.state);
    let guard = tandem_providers::provider_auth_mutation_in_dir(&directory)
        .await
        .unwrap();
    let observed = Arc::new(tokio::sync::Notify::new());
    let lookup = crate::solution_installation::scope_model_account_observation(
        observed.clone(),
        f.state.authorize_solution_model_account(
            &f.verified,
            &f.configuration.configuration.scope,
            BINDING,
        ),
    );
    tokio::pin!(lookup);
    // A test-only notification proves that the initiating grant check allowed
    // this operation. The actual credential file lock still prevents completion.
    tokio::select! {
        result = &mut lookup => panic!("lookup completed while mutation lock held: {result:?}"),
        _ = observed.notified() => {},
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => panic!("initial account grant check was not reached"),
    }
    f.state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .remove("model-eng");
    drop(guard);
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), lookup)
        .await
        .unwrap();
    assert!(result.unwrap_err().to_string().contains("current user"));
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_service_model_account_preview_and_stage_require_current_account() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            let mut f = Fixture::new().await;
            f.state.memory_db_path = f.root.path().join("memory.sqlite");
            let revision = stored_key(&f, TOKEN).await;
            configure(&mut f, &revision, TOKEN).await;

            // A separately signed synthetic pack explicitly permits this HTTP
            // provider. The shipped local-only diagnostic fixture is unchanged.
            let mut entries = fixture();
            let (_, source) = entries
                .iter_mut()
                .find(|(path, _)| path == "solution.json")
                .unwrap();
            let mut blueprint: serde_json::Value = serde_json::from_str(source).unwrap();
            blueprint["constraints"]["allowed_providers"] = serde_json::json!(["llama_cpp"]);
            blueprint["constraints"]["allow_network_egress"] = true.into();
            *source = serde_json::to_string(&blueprint).unwrap();
            let archive = f.root.path().join("model-solution.zip");
            let key = signed(&archive, &entries);
            let _keys = EnvGuard::set("TANDEM_PACK_TRUSTED_PUBLIC_KEYS", &key);
            f.state.pack_manager = Arc::new(PackManager::new(f.root.path().join("model-packs")));
            f.state
                .pack_manager
                .install(request(&archive))
                .await
                .unwrap();
            f.configuration.configuration.constraints.allowed_providers =
                std::collections::BTreeSet::from(["llama_cpp".into()]);
            f.configuration
                .configuration
                .constraints
                .allow_network_egress = true;

            let denied = f
                .state
                .preview_solution_configuration(&f.verified, &f.configuration)
                .await
                .err()
                .expect("catalog membership must not approve the account");
            assert!(denied.to_string().contains("model_binding_unapproved"));
            grant(&f, "model-eng", "eng").await;

            // An unrelated denied account must not block the selected allowed
            // model. It is still omitted from the host's approved model facts.
            let mut cli = f.state.config.get_layers_value().await["cli"].clone();
            let mut unused = cli["solution_installation"]["models"][BINDING].clone();
            unused["account"]["authorization_revision"] = "not-the-reviewed-revision".into();
            cli["solution_installation"]["models"]["unused.denied"] = unused;
            let mut runtime = f.state.runtime.wait().clone();
            runtime.config = tandem_core::ConfigStore::new(
                f.root.path().join("unrelated-model-config.json"),
                Some(cli),
            )
            .await
            .unwrap();
            f.state.runtime = Arc::new(std::sync::OnceLock::from(runtime));
            let mut reviewed = f.review_and_save().await;

            f.state
                .enterprise
                .org_unit_access_grants
                .write()
                .await
                .remove("model-eng");
            let denied = f
                .state
                .stage_solution_installation(&f.verified, reviewed.clone())
                .await
                .unwrap_err();
            assert!(denied.to_string().contains("model_binding_unapproved"));
            assert!(!f.root.path().join(".tandem/agent-team/templates").exists());

            grant(&f, "model-eng", "eng").await;
            let reconnected = stored_key(&f, TOKEN).await;
            assert_ne!(reconnected, revision);
            assert!(f
                .state
                .preview_solution_configuration(&f.verified, &f.configuration)
                .await
                .is_err());
            assert!(f
                .state
                .stage_solution_installation(&f.verified, reviewed.clone())
                .await
                .is_err());
            configure(&mut f, &reconnected, TOKEN).await;
            // The customer document is unchanged. Review the new host account
            // facts against its existing protected configuration version.
            reviewed.reviewed_composition = f
                .state
                .preview_solution_configuration(&f.verified, &f.configuration)
                .await
                .unwrap()
                .composition_sha256;
            let staged = f
                .state
                .stage_solution_installation(&f.verified, reviewed)
                .await
                .unwrap();
            assert!(staged.all_components_staged());
            let agent = &staged.plan.components["central-brain"].resource_id;
            let template = f
                .root
                .path()
                .join(".tandem/agent-team/templates")
                .join(format!("{agent}.yaml"));
            let observed: tandem_orchestrator::AgentTemplate =
                serde_json::from_slice(&std::fs::read(template).unwrap()).unwrap();
            assert!(!observed.enabled);
        },
    )
    .await;
}

#[tokio::test]
#[serial_test::serial(pack_signature_env)]
async fn solution_service_current_route_requires_operator_review_live_probe_and_account() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x52; 32]),
        None,
        async {
            use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let offered = Arc::new(AtomicBool::new(true));
            let server_calls = calls.clone();
            let server_offered = offered.clone();
            let server = tokio::spawn(async move {
                while let Ok((mut socket, _)) = listener.accept().await {
                    let mut request = [0u8; 4096];
                    let size = socket.read(&mut request).await.unwrap();
                    let request = String::from_utf8_lossy(&request[..size]);
                    assert!(request.starts_with("GET /v1/models HTTP/1.1"));
                    server_calls.fetch_add(1, Ordering::SeqCst);
                    let model = if server_offered.load(Ordering::SeqCst) {
                        "synthetic-model"
                    } else {
                        "other-model"
                    };
                    let body = format!(r#"{{"data":[{{"id":"{model}"}}]}}"#);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    socket.write_all(response.as_bytes()).await.unwrap();
                }
            });
            let mut f = Fixture::new().await;
            f.state.memory_db_path = f.root.path().join("memory.sqlite");
            let revision = stored_key(&f, TOKEN).await;
            configure_at(
                &mut f,
                &revision,
                TOKEN,
                &format!("http://{address}/v1"),
            )
            .await;
            let _keys = install_network_profile_pack(&mut f).await;
            grant(&f, "model-eng", "eng").await;

            let review_expiry = crate::now_ms() + 300_000;
            set_route_review(&mut f, &revision, review_expiry).await;
            let request = f.review_and_save().await;
            let staged = f
                .state
                .stage_solution_installation(&f.verified, request)
                .await
                .unwrap();
            // Removing reviewed facts after staging cannot be repaired by a
            // customer override or the still-configured provider catalog.
            clear_route_review(&mut f).await;
            assert!(observe_private_route(
                &f,
                &f.verified,
                staged.generation,
                &staged.composition_sha256,
            )
            .await
            .is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 0);

            set_route_review(&mut f, &revision, review_expiry).await;
            let route = observe_private_route(
                &f,
                &f.verified,
                staged.generation,
                &staged.composition_sha256,
            )
            .await
            .unwrap();
            assert_eq!(route.binding.binding_id, BINDING);
            assert_eq!(route.processing_regions, ["local".into()].into());
            assert_eq!(calls.load(Ordering::SeqCst), 1);

            let catalog = f
                .state
                .load_current_staged_model_profile_catalog(
                    &f.verified,
                    &f.configuration.configuration.scope,
                    staged.generation,
                    &staged.composition_sha256,
                )
                .await
                .unwrap();
            let routes = std::collections::BTreeMap::from([(BINDING.into(), route.clone())]);
            let bindings = std::collections::BTreeMap::from([(
                "economy".into(),
                BINDING.into(),
            )]);
            let data_classes = [DataClass::Internal];
            let modalities = [tandem_solutions::ModelModality::Text].into();
            let policies = [tandem_solutions::ModelDataPolicy {
                data_class: DataClass::Internal,
                allowed_providers: ["llama_cpp".into()].into(),
                allowed_processing_regions: ["local".into()].into(),
                max_retention_hours: 2,
                allow_network: true,
            }];
            let after_probe_expiry = route.available_until_ms.unwrap() + 1;
            assert_eq!(
                tandem_solutions::resolve_model_profile(
                    tandem_solutions::ModelProfileResolutionInput {
                        catalog: &catalog.catalog,
                        requested_class: None,
                        escalation_from: None,
                        escalation_reason: None,
                        customer_bindings: &bindings,
                        current_routes: &routes,
                        constraints: &staged.plan.constraints,
                        data_policies: &policies,
                        data_classes: &data_classes,
                        modalities: &modalities,
                        uses_tools: false,
                        maximum_input_tokens: 1,
                        maximum_output_tokens: 1,
                        inherited_limits: &catalog.catalog.profiles["economy"].limits,
                        previously_selected: &[],
                        previous_evaluations: 0,
                        expected_catalog_sha256: None,
                        started_at_ms: after_probe_expiry - 1,
                        now_ms: after_probe_expiry,
                    },
                )
                .unwrap_err()
                .code,
                "model_unavailable"
            );

            // Ordinary customer/project overrides cannot replace CLI review.
            f.state
                .config
                .patch_project(serde_json::json!({"solution_installation": {
                    "models": {BINDING: {"review": {
                        "revision": "forged", "supports_tool_use": true,
                        "processing_regions": ["anywhere"]
                    }}}
                }}))
                .await
                .unwrap();
            let unchanged = observe_private_route(
                &f,
                &f.verified,
                staged.generation,
                &staged.composition_sha256,
            )
            .await
            .unwrap();
            assert_eq!(unchanged.binding_revision, route.binding_revision);
            assert!(!unchanged.supports_tool_use);
            assert_eq!(calls.load(Ordering::SeqCst), 2);

            offered.store(false, Ordering::SeqCst);
            assert!(observe_private_route(
                &f,
                &f.verified,
                staged.generation,
                &staged.composition_sha256,
            )
            .await
            .is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            offered.store(true, Ordering::SeqCst);

            f.state
                .enterprise
                .org_unit_access_grants
                .write()
                .await
                .remove("model-eng");
            assert!(observe_private_route(
                &f,
                &f.verified,
                staged.generation,
                &staged.composition_sha256,
            )
            .await
            .is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            grant(&f, "model-eng", "eng").await;

            set_route_review(&mut f, &revision, crate::now_ms() - 1).await;
            assert!(observe_private_route(
                &f,
                &f.verified,
                staged.generation,
                &staged.composition_sha256,
            )
            .await
            .is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            set_route_review(&mut f, &revision, review_expiry).await;

            // Endpoint rebinding and credential reconnect invalidate review
            // before the model-list endpoint can issue a new observation.
            f.state
                .providers
                .reload(serde_json::from_value(serde_json::json!({
                    "default_provider": "llama_cpp",
                    "providers": {"llama_cpp": {
                        "url": "http://127.0.0.1:9/v1",
                        "api_key": TOKEN,
                        "default_model": "synthetic-model"
                    }}
                }))
                .unwrap())
                .await;
            assert!(observe_private_route(
                &f,
                &f.verified,
                staged.generation,
                &staged.composition_sha256,
            )
            .await
            .is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            stored_key(&f, TOKEN).await;
            assert!(observe_private_route(
                &f,
                &f.verified,
                staged.generation,
                &staged.composition_sha256,
            )
            .await
            .is_err());
            assert_eq!(calls.load(Ordering::SeqCst), 3);
            server.abort();
        },
    )
    .await;
}
