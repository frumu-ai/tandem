// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use axum::{body::Body, http::Request, response::Response, routing::post, Router};
use base64::Engine as _;
use std::time::Duration;
use tandem_enterprise_contract::{
    hosted_policy::role_capabilities, AuthorityChain, HumanActor, RequestPrincipal,
    TenantContextAssertionClaims,
};
use tower::ServiceExt;

struct KeyringEnv(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl KeyringEnv {
    fn set(raw: &str) -> Self {
        let names = [
            "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
            "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE",
        ];
        let restore = Self(
            names
                .iter()
                .map(|name| (*name, std::env::var_os(name)))
                .collect(),
        );
        std::env::set_var(names[0], raw);
        std::env::remove_var(names[1]);
        restore
    }
}

impl Drop for KeyringEnv {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..) {
            if let Some(value) = value {
                std::env::set_var(name, value);
            } else {
                std::env::remove_var(name);
            }
        }
    }
}

fn key(seed: u8) -> Value {
    json!({
        "purpose": "context_assertion",
        "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            ed25519_dalek::SigningKey::from_bytes(&[seed; 32]).verifying_key().to_bytes(),
        ),
        "organization_id": "org-a", "deployment_id": "dep-a",
        "allowed_audiences": ["tandem-runtime"], "status": "active"
    })
}

struct Fixture {
    root: tempfile::TempDir,
    state: AppState,
    actor: VerifiedTenantContext,
    previous: Arc<crate::context_assertion_security::RuntimeContextAssertionSecurity>,
    _env: KeyringEnv,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut state = crate::test_support::test_state().await;
        state.protected_audit_path = root.path().join("audit.jsonl");
        state.enterprise.hosted_policy_revision_path = root.path().join("policy-revision.json");
        let previous = Arc::new(crate::context_assertion_security::RuntimeContextAssertionSecurity::from_test_metadata_keyring(
            &json!({"old": key(81)}).to_string(), &root.path().join("replay.json"),
        ));
        *state.context_assertion_security.write().unwrap() = Some(previous.clone());
        let env = KeyringEnv::set(&json!({"old": key(81), "new": key(82)}).to_string());
        let now = crate::now_ms();
        let tenant =
            TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "alice");
        let mut claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            now,
            now + 300_000,
            "reload-admin",
            tenant,
            HumanActor::tandem_user("alice"),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                "alice",
                "tandem-web",
            )),
            vec!["hosted:role:admin".into()],
        );
        claims.policy_version = Some(1);
        claims.capabilities = role_capabilities("admin")
            .into_iter()
            .map(str::to_owned)
            .collect();
        let fixture = Self {
            root,
            state,
            actor: claims.into(),
            previous,
            _env: env,
        };
        fixture.write_policy(1, true);
        fixture.state.reload_hosted_policy().await.unwrap();
        fixture
    }

    fn write_policy(&self, version: u64, active: bool) {
        let path = self.root.path().join("policy.json");
        std::fs::write(&path, serde_json::to_vec(&json!({
            "schema_version": 1, "policy_version": version,
            "organization_id": "org-a", "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64).unwrap(),
            "users": [{"id": "alice", "email": null, "username": null, "role": "admin",
                "capabilities": role_capabilities("admin"), "is_active": active, "email_verified": true}],
            "org_units": [], "org_unit_memberships": [], "deployment_grants": []
        })).unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        self.state
            .enterprise
            .hosted_policy
            .configure_test_source("org-a", "dep-a", path);
    }

    fn unchanged(&self) -> bool {
        Arc::ptr_eq(
            &self.previous,
            &self.state.context_assertion_security_snapshot().unwrap(),
        )
    }
}

fn start_reload(fixture: &Fixture) -> tokio::task::JoinHandle<Response> {
    let state = fixture.state.clone();
    let actor = fixture.actor.clone();
    tokio::spawn(crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async move {
            Router::new()
                .route("/admin/context-assertions/reload", post(reload))
                .layer(Extension(actor.tenant_context.clone()))
                .layer(Extension(actor))
                .layer(Extension(
                    super::super::host_authority::RequestLocality::default(),
                ))
                .with_state(state)
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/admin/context-assertions/reload")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap()
        },
    ))
}

async fn wait_for_prepared(fixture: &Fixture) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let prepared = crate::audit::try_load_protected_audit_events_for_tenant(
                &fixture.state,
                &fixture.actor.tenant_context,
            )
            .await
            .is_ok_and(|events| {
                events
                    .iter()
                    .any(|event| event.event_type == "context_assertion.verifier_reload_prepared")
            });
            if prepared {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("real reload must prepare its protected audit event");
}

#[tokio::test]
#[serial_test::serial(context_assertion_env)]
async fn keyring_reload_rejects_revocation_published_during_publication_wait() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            let fixture = Fixture::new().await;
            let publication = fixture.state.lock_hosted_policy_publication().await;
            fixture.write_policy(2, false);
            let mut revoke = Box::pin(fixture.state.reload_hosted_policy());
            assert!(futures::poll!(&mut revoke).is_pending());
            let request = start_reload(&fixture);
            wait_for_prepared(&fixture).await;
            assert!(
                fixture.unchanged(),
                "a prepared request must await publication authority"
            );
            drop(publication);
            revoke.await.unwrap();
            let response = tokio::time::timeout(Duration::from_secs(10), request)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert!(fixture.unchanged());
        },
    )
    .await;
}

#[tokio::test]
#[serial_test::serial(context_assertion_env)]
async fn keyring_reload_rechecks_expiry_after_publication_wait() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            let mut fixture = Fixture::new().await;
            let publication = fixture.state.lock_hosted_policy_publication().await;
            fixture.actor.expires_at_ms = crate::now_ms() + 5_000;
            let request = start_reload(&fixture);
            wait_for_prepared(&fixture).await;
            let remaining = fixture.actor.expires_at_ms.saturating_sub(crate::now_ms());
            tokio::time::sleep(Duration::from_millis(remaining + 1)).await;
            drop(publication);
            let response = tokio::time::timeout(Duration::from_secs(10), request)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert!(fixture.unchanged());
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial(context_assertion_env)]
async fn keyring_reload_blocks_policy_publication_through_verifier_writer_wait_and_swap() {
    crate::encrypted_file_store::with_test_crypto_provider(
        tandem_memory::MemoryCryptoProvider::local_key([0x39; 32]),
        None,
        async {
            let fixture = Fixture::new().await;
            let publication = fixture.state.lock_hosted_policy_publication().await;
            let request = start_reload(&fixture);
            wait_for_prepared(&fixture).await;
            // Preparation first reads the previous verifier. Hold its writer
            // only after that read, then let the final publisher proceed.
            let state = fixture.state.clone();
            let (acquired, waiting) = tokio::sync::oneshot::channel();
            let (release, released) = std::sync::mpsc::channel();
            let writer = std::thread::spawn(move || {
                let _writer = state.context_assertion_security.write().unwrap();
                acquired.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(20)).unwrap();
            });
            waiting.await.unwrap();
            drop(publication);
            tokio::time::timeout(Duration::from_secs(10), async {
                while !fixture
                    .state
                    .enterprise
                    .hosted_policy
                    .publication_mutex_locked_for_test()
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect(
                "reload must acquire policy publication before waiting for the verifier writer",
            );
            fixture.write_policy(2, false);
            let mut revoke = Box::pin(fixture.state.reload_hosted_policy());
            assert!(futures::poll!(&mut revoke).is_pending());
            release.send(()).unwrap();
            writer.join().unwrap();
            tokio::time::timeout(Duration::from_secs(10), revoke)
                .await
                .unwrap()
                .unwrap();
            let response = tokio::time::timeout(Duration::from_secs(10), request)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(
                !fixture.unchanged(),
                "the authorized swap completes before revocation can publish"
            );
            assert_eq!(
                fixture
                    .state
                    .context_assertion_security_snapshot()
                    .unwrap()
                    .key_count(),
                2
            );
        },
    )
    .await;
}
