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
    runtime.providers = ProviderRegistry::new(serde_json::from_value(serde_json::json!({
        "default_provider": "llama_cpp", "providers": {"llama_cpp": {
            "url": "http://127.0.0.1:9/v1", "api_key": token, "default_model": "synthetic-model"}}
    })).unwrap());
    fixture.state.runtime = Arc::new(std::sync::OnceLock::from(runtime));
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
