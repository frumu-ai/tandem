// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn remote_catalog_support_message(provider_id: &str) -> String {
    match provider_id {
        "openai-codex" => {
            "Codex account models use a Tandem starter catalog. Live discovery is not enabled here."
                .to_string()
        }
        "azure" | "vertex" | "bedrock" | "copilot" | "ollama" => {
            "Live model discovery is unavailable for this provider. Enter a model ID manually."
                .to_string()
        }
        "anthropic" | "cohere" => {
            "This provider does not currently expose a reliable live model catalog here. Enter a model ID manually."
                .to_string()
        }
        _ => "Live model discovery is unavailable. Enter a model ID manually.".to_string(),
    }
}

fn provider_config_api_key(
    cfg: &Value,
    provider_id: &str,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Option<String> {
    allow_shared_auth_sources
        .then(|| {
            provider_config_value(cfg, provider_id)
                .and_then(|entry| entry.get("api_key").or_else(|| entry.get("apiKey")))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty() && *value != "x")
                .map(str::to_string)
        })
        .flatten()
        .or_else(|| {
            runtime_auth
                .get(&provider_id.to_ascii_lowercase())
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            persisted_auth
                .get(&provider_id.to_ascii_lowercase())
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            allow_shared_auth_sources
                .then(|| {
                    provider_env_candidates(provider_id)
                        .into_iter()
                        .find_map(|key| std::env::var(&key).ok())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                })
                .flatten()
        })
}

#[cfg(test)]
mod provider_auth_resolution_tests {
    use super::*;

    fn verified_deployment_admin(
        tenant_context: tandem_types::TenantContext,
        actor_id: &str,
    ) -> tandem_types::VerifiedTenantContext {
        let now = crate::now_ms();
        tandem_types::VerifiedTenantContext {
            tenant_context,
            human_actor: tandem_types::HumanActor::tandem_user(actor_id),
            authority_chain: tandem_types::AuthorityChain::from_request(
                tandem_types::RequestPrincipal::authenticated_user(actor_id, "provider-test"),
            ),
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: vec!["deployment.admin".to_string()],
            policy_version: None,
            strict_projection: None,
            issuer: "provider-test".to_string(),
            audience: "tandem".to_string(),
            issued_at_ms: now,
            expires_at_ms: now.saturating_add(60_000),
            assertion_id: format!("provider-test-{actor_id}-{now}"),
            assertion_key_id: None,
        }
    }

    fn codex_oauth(managed_by: &str, refresh_token: &str) -> tandem_core::OAuthProviderCredential {
        tandem_core::OAuthProviderCredential {
            provider_id: OPENAI_CODEX_PROVIDER_ID.to_string(),
            access_token: "access-token".to_string(),
            refresh_token: refresh_token.to_string(),
            expires_at_ms: 1,
            account_id: Some("acct".to_string()),
            email: Some("operator@example.com".to_string()),
            display_name: Some("Operator".to_string()),
            managed_by: managed_by.to_string(),
            api_key: None,
        }
    }

    #[test]
    fn codex_cli_oauth_credentials_can_refresh_in_process() {
        assert!(openai_codex_oauth_refreshable_in_process(&codex_oauth(
            "tandem",
            "refresh-token"
        )));
        assert!(openai_codex_oauth_refreshable_in_process(&codex_oauth(
            "codex-cli",
            "refresh-token"
        )));
        assert!(!openai_codex_oauth_refreshable_in_process(&codex_oauth(
            "codex-upload",
            "refresh-token"
        )));
        assert!(!openai_codex_oauth_refreshable_in_process(&codex_oauth(
            "codex-cli",
            "   "
        )));
    }

    #[test]
    #[serial_test::serial]
    fn provider_api_key_resolution_preserves_runtime_precedence_over_env_fallback() {
        let provider_id = format!("review-precedence-{}", uuid::Uuid::new_v4());
        let env_key = provider_env_candidates(&provider_id)
            .into_iter()
            .next()
            .expect("provider env key");
        std::env::set_var(&env_key, "env-key");

        let runtime_auth =
            HashMap::from([(provider_id.to_ascii_lowercase(), "runtime-key".to_string())]);
        let persisted_auth = HashMap::from([(
            provider_id.to_ascii_lowercase(),
            "persisted-key".to_string(),
        )]);
        assert_eq!(
            provider_config_api_key(
                &json!({}),
                &provider_id,
                &runtime_auth,
                &persisted_auth,
                true
            )
            .as_deref(),
            Some("runtime-key")
        );
        assert_eq!(
            provider_config_api_key(
                &json!({}),
                &provider_id,
                &HashMap::new(),
                &persisted_auth,
                true
            )
            .as_deref(),
            Some("persisted-key")
        );
        assert_eq!(
            provider_config_api_key(
                &json!({}),
                &provider_id,
                &HashMap::new(),
                &persisted_auth,
                false
            )
            .as_deref(),
            Some("persisted-key"),
            "explicit tenants may use only their tenant-scoped persisted key"
        );
        assert!(
            provider_config_api_key(
                &json!({}),
                &provider_id,
                &HashMap::new(),
                &HashMap::new(),
                false,
            )
            .is_none(),
            "explicit tenants must not use shared env fallback keys"
        );

        std::env::remove_var(env_key);
    }

    #[test]
    fn openai_codex_default_model_validation_rejects_retired_models() {
        use tandem_providers::{
            openai_codex_effective_default_model, openai_codex_model_is_supported,
        };
        // The retired phantom `gpt-5.1-codex-max` must not be honored as a saved
        // default; a stale config falls back to the compiled default so channel
        // runs (which read this default fresh) stop hitting a provider 400.
        assert!(!openai_codex_model_is_supported("gpt-5.1-codex-max"));
        assert!(!openai_codex_model_is_supported("gpt-5.6"));
        assert!(openai_codex_model_is_supported("gpt-5.6-sol"));
        assert!(openai_codex_model_is_supported("gpt-5.6-terra"));
        assert!(openai_codex_model_is_supported("gpt-5.6-luna"));
        assert!(openai_codex_model_is_supported("gpt-5.5"));
        // The compiled fallback must itself always be a supported model.
        assert!(openai_codex_model_is_supported(OPENAI_CODEX_DEFAULT_MODEL));
        // A retired saved default heals to the compiled default; a valid one is kept.
        assert_eq!(
            openai_codex_effective_default_model(Some("gpt-5.1-codex-max")),
            OPENAI_CODEX_DEFAULT_MODEL
        );
        assert_eq!(
            openai_codex_effective_default_model(Some("gpt-5.6-sol")),
            "gpt-5.6-sol"
        );
        assert_eq!(
            openai_codex_effective_default_model(None),
            OPENAI_CODEX_DEFAULT_MODEL
        );
    }

    fn scoped_codex_oauth(label: &str, expires_at_ms: u64) -> tandem_core::OAuthProviderCredential {
        tandem_core::OAuthProviderCredential {
            provider_id: OPENAI_CODEX_PROVIDER_ID.to_string(),
            access_token: format!("access-{label}"),
            refresh_token: format!("refresh-{label}"),
            expires_at_ms,
            account_id: Some(format!("account-{label}")),
            email: None,
            display_name: None,
            managed_by: "tandem".to_string(),
            api_key: Some(format!("api-{label}")),
        }
    }

    fn save_scoped_codex_oauth(
        state: &AppState,
        tenant_context: &TenantContext,
        credential: tandem_core::OAuthProviderCredential,
    ) {
        tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
            &provider_auth_security_dir_for_state(state),
            tenant_context,
            OPENAI_CODEX_PROVIDER_ID,
            credential,
        )
        .expect("save scoped Codex OAuth credential");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn proactive_refresh_discovers_local_and_two_hosted_tenants() {
        let state = crate::test_support::test_state().await;
        let local = TenantContext::local_implicit();
        let tenant_a = TenantContext::explicit("org-a", "workspace-a", None);
        let mut tenant_b = TenantContext::explicit("org-b", "workspace-b", None);
        tenant_b.deployment_id = Some("deployment-b".to_string());
        let fresh_expiry = crate::now_ms().saturating_add(60 * 60 * 1000);
        for (tenant, label) in [(&local, "local"), (&tenant_a, "a"), (&tenant_b, "b")] {
            save_scoped_codex_oauth(&state, tenant, scoped_codex_oauth(label, fresh_expiry));
        }

        let tenants = openai_codex_refreshable_tenants(&state);
        assert_eq!(tenants.len(), 3);
        state.refresh_provider_oauth_once().await;
        for tenant in [&local, &tenant_a, &tenant_b] {
            assert!(
                state
                    .providers
                    .tenant_provider_auth_is_loaded(tenant, OPENAI_CODEX_PROVIDER_ID)
                    .await
            );
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn expired_credentials_refresh_independently_for_local_and_hosted_tenants() {
        let state = crate::test_support::test_state().await;
        let local = TenantContext::local_implicit();
        let tenant_a = TenantContext::explicit("org-a", "workspace-a", None);
        let tenant_b = TenantContext::explicit("org-b", "workspace-b", None);
        for (tenant, label) in [(&local, "local"), (&tenant_a, "a"), (&tenant_b, "b")] {
            save_scoped_codex_oauth(&state, tenant, scoped_codex_oauth(label, 1));
            let refreshed_label = label.to_string();
            refresh_openai_codex_oauth_with(&state, tenant, true, move |mut credential| {
                let refreshed_label = refreshed_label.clone();
                async move {
                    credential.access_token = format!("refreshed-access-{refreshed_label}");
                    credential.api_key = Some(format!("refreshed-api-{refreshed_label}"));
                    credential.expires_at_ms = crate::now_ms().saturating_add(60 * 60 * 1000);
                    Ok(credential)
                }
            })
            .await
            .expect("refresh scoped credential");
        }

        let security_dir = provider_auth_security_dir_for_state(&state);
        for (tenant, label) in [(&local, "local"), (&tenant_a, "a"), (&tenant_b, "b")] {
            let credential = tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
                &security_dir,
                tenant,
                OPENAI_CODEX_PROVIDER_ID,
            )
            .expect("refreshed scoped credential");
            assert_eq!(credential.access_token, format!("refreshed-access-{label}"));
            assert!(
                state
                    .providers
                    .tenant_provider_auth_is_loaded(tenant, OPENAI_CODEX_PROVIDER_ID)
                    .await
            );
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn concurrent_forced_refreshes_for_one_tenant_coalesce() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit("org-coalesce", "workspace", None);
        save_scoped_codex_oauth(&state, &tenant, scoped_codex_oauth("coalesce", 1));
        let refresh_count = Arc::new(AtomicUsize::new(0));

        let first_state = state.clone();
        let first_tenant = tenant.clone();
        let first_count = refresh_count.clone();
        let first = tokio::spawn(async move {
            refresh_openai_codex_oauth_with(
                &first_state,
                &first_tenant,
                true,
                move |mut credential| {
                    let first_count = first_count.clone();
                    async move {
                        first_count.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        credential.access_token = "coalesced-access".to_string();
                        credential.expires_at_ms = crate::now_ms().saturating_add(60 * 60 * 1000);
                        Ok(credential)
                    }
                },
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while refresh_count.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first refresh reached exchange");

        let second_state = state.clone();
        let second_tenant = tenant.clone();
        let second_count = refresh_count.clone();
        let second = tokio::spawn(async move {
            refresh_openai_codex_oauth_with(
                &second_state,
                &second_tenant,
                true,
                move |credential| {
                    let second_count = second_count.clone();
                    async move {
                        second_count.fetch_add(1, Ordering::SeqCst);
                        Ok(credential)
                    }
                },
            )
            .await
        });

        first.await.expect("first task").expect("first refresh");
        second.await.expect("second task").expect("second refresh");
        assert_eq!(refresh_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn concurrent_hosted_refreshes_preserve_both_tenant_credentials() {
        let state = crate::test_support::test_state().await;
        let tenant_a = TenantContext::explicit("org-concurrent-a", "workspace", None);
        let tenant_b = TenantContext::explicit("org-concurrent-b", "workspace", None);
        save_scoped_codex_oauth(&state, &tenant_a, scoped_codex_oauth("concurrent-a", 1));
        save_scoped_codex_oauth(&state, &tenant_b, scoped_codex_oauth("concurrent-b", 1));

        let refresh_a =
            refresh_openai_codex_oauth_with(&state, &tenant_a, true, |mut credential| async move {
                tokio::task::yield_now().await;
                credential.access_token = "refreshed-concurrent-a".to_string();
                credential.expires_at_ms = crate::now_ms().saturating_add(60 * 60 * 1000);
                Ok(credential)
            });
        let refresh_b =
            refresh_openai_codex_oauth_with(&state, &tenant_b, true, |mut credential| async move {
                tokio::task::yield_now().await;
                credential.access_token = "refreshed-concurrent-b".to_string();
                credential.expires_at_ms = crate::now_ms().saturating_add(60 * 60 * 1000);
                Ok(credential)
            });
        let (result_a, result_b) = tokio::join!(refresh_a, refresh_b);
        result_a.expect("tenant A refresh");
        result_b.expect("tenant B refresh");

        let security_dir = provider_auth_security_dir_for_state(&state);
        let stored_a = tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
            &security_dir,
            &tenant_a,
            OPENAI_CODEX_PROVIDER_ID,
        )
        .expect("tenant A credential");
        let stored_b = tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
            &security_dir,
            &tenant_b,
            OPENAI_CODEX_PROVIDER_ID,
        )
        .expect("tenant B credential");
        assert_eq!(stored_a.access_token, "refreshed-concurrent-a");
        assert_eq!(stored_b.access_token, "refreshed-concurrent-b");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn automatic_refresh_audit_failure_restores_persisted_and_runtime_credential() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::local_implicit();
        let prior = scoped_codex_oauth("audit-prior", 1);
        save_scoped_codex_oauth(&state, &tenant, prior.clone());
        state.auth.write().await.insert(
            OPENAI_CODEX_PROVIDER_ID.to_string(),
            "api-audit-prior".to_string(),
        );
        load_openai_codex_oauth_into_runtime(&state, &tenant)
            .await
            .expect("load prior runtime credential");
        tokio::fs::create_dir_all(&state.protected_audit_path)
            .await
            .expect("make protected audit path unwritable as a file");
        crate::audit::reset_protected_audit_tail_for_test(&state.protected_audit_path).await;

        let result =
            refresh_openai_codex_oauth_with(&state, &tenant, true, |mut credential| async move {
                credential.access_token = "refreshed-audit-failure".to_string();
                credential.api_key = Some("api-refreshed-audit-failure".to_string());
                credential.expires_at_ms = crate::now_ms().saturating_add(60 * 60 * 1000);
                Ok(credential)
            })
            .await;

        assert!(
            result.is_err(),
            "refresh must fail when required audit append fails"
        );
        assert_eq!(openai_codex_oauth_credential(&state, &tenant), Some(prior));
        assert_eq!(
            state
                .auth
                .read()
                .await
                .get(OPENAI_CODEX_PROVIDER_ID)
                .map(String::as_str),
            Some("api-audit-prior")
        );
        assert!(
            state
                .providers
                .tenant_provider_auth_is_loaded(&tenant, OPENAI_CODEX_PROVIDER_ID)
                .await,
            "the prior runtime bearer credential must remain loaded"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn audit_rollback_cas_preserves_newer_replacement_and_disconnect_credentials() {
        let state = crate::test_support::test_state().await;
        let replacement_tenant =
            TenantContext::explicit("org-audit-cas-replacement", "workspace", None);
        let replacement_prior = scoped_codex_oauth("replacement-prior", 1);
        let replacement_committed = scoped_codex_oauth("replacement-committed", 2);
        let mut replacement_newer = scoped_codex_oauth("replacement-newer", 3);
        replacement_newer.access_token = replacement_committed.access_token.clone();
        save_scoped_codex_oauth(&state, &replacement_tenant, replacement_prior);

        let replacement_snapshot =
            snapshot_openai_codex_oauth_mutation(&state, &replacement_tenant).await;
        persist_openai_codex_oauth_credential(&state, &replacement_tenant, replacement_committed)
            .await
            .expect("commit connect/import credential");
        save_scoped_codex_oauth(&state, &replacement_tenant, replacement_newer.clone());
        restore_openai_codex_oauth_mutation(&state, &replacement_tenant, &replacement_snapshot)
            .await
            .expect("skip stale connect/import rollback");
        assert_eq!(
            openai_codex_oauth_credential(&state, &replacement_tenant),
            Some(replacement_newer),
            "connect/import rollback must compare the complete committed credential"
        );

        let disconnect_tenant =
            TenantContext::explicit("org-audit-cas-disconnect", "workspace", None);
        let disconnect_prior = scoped_codex_oauth("disconnect-prior", 1);
        let disconnect_newer = scoped_codex_oauth("disconnect-newer", 2);
        save_scoped_codex_oauth(&state, &disconnect_tenant, disconnect_prior);

        let disconnect_snapshot =
            snapshot_openai_codex_oauth_mutation(&state, &disconnect_tenant).await;
        assert!(
            delete_openai_codex_oauth_credential(&state, &disconnect_tenant)
                .await
                .expect("commit disconnect")
        );
        save_scoped_codex_oauth(&state, &disconnect_tenant, disconnect_newer.clone());
        restore_openai_codex_oauth_mutation(&state, &disconnect_tenant, &disconnect_snapshot)
            .await
            .expect("skip stale disconnect rollback");
        assert_eq!(
            openai_codex_oauth_credential(&state, &disconnect_tenant),
            Some(disconnect_newer),
            "disconnect rollback must not replace a newer credential"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn disconnect_waits_for_in_flight_refresh_and_cannot_be_resurrected() {
        let state = crate::test_support::test_state().await;
        let actor_id = "admin-refresh-disconnect";
        let tenant = TenantContext::explicit(
            "org-refresh-disconnect",
            "workspace",
            Some(actor_id.to_string()),
        );
        let verified = verified_deployment_admin(tenant.clone(), actor_id);
        save_scoped_codex_oauth(&state, &tenant, scoped_codex_oauth("race", 1));
        let (refresh_entered_tx, refresh_entered_rx) = tokio::sync::oneshot::channel();
        let (release_refresh_tx, release_refresh_rx) = tokio::sync::oneshot::channel();

        let refresh_state = state.clone();
        let refresh_tenant = tenant.clone();
        let refresh_task = tokio::spawn(async move {
            refresh_openai_codex_oauth_with(
                &refresh_state,
                &refresh_tenant,
                true,
                move |mut credential| async move {
                    let _ = refresh_entered_tx.send(());
                    let _ = release_refresh_rx.await;
                    credential.access_token = "refreshed-before-disconnect".to_string();
                    credential.expires_at_ms = crate::now_ms().saturating_add(60 * 60 * 1000);
                    Ok(credential)
                },
            )
            .await
        });
        refresh_entered_rx.await.expect("refresh entered exchange");

        let disconnect_state = state.clone();
        let disconnect_tenant = tenant.clone();
        let mut disconnect_task = tokio::spawn(async move {
            provider_oauth_disconnect(
                State(disconnect_state),
                Extension(disconnect_tenant),
                Extension(RequestPrincipal::anonymous()),
                Extension(crate::http::host_authority::RequestLocality::from_peer_and_headers(
                    Some("127.0.0.1:43210".parse().expect("loopback peer")),
                    &axum::http::HeaderMap::new(),
                )),
                Some(Extension(verified)),
                Path(OPENAI_CODEX_PROVIDER_ID.to_string()),
            )
            .await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut disconnect_task)
                .await
                .is_err(),
            "disconnect must serialize behind the in-flight refresh transaction"
        );

        release_refresh_tx.send(()).expect("release refresh");
        refresh_task
            .await
            .expect("refresh task")
            .expect("refresh result");
        disconnect_task.await.expect("disconnect task");

        assert!(
            openai_codex_oauth_credential(&state, &tenant).is_none(),
            "the completed refresh must not recreate the disconnected credential"
        );
        assert!(
            !state
                .providers
                .tenant_provider_auth_is_loaded(&tenant, OPENAI_CODEX_PROVIDER_ID)
                .await,
            "the disconnected credential must also stay absent from runtime"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn revoked_refresh_emits_sanitized_failure_and_reauth_events() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit("org-revoked", "workspace", None);
        save_scoped_codex_oauth(&state, &tenant, scoped_codex_oauth("revoked", 1));
        let mut events = state.event_bus.subscribe();
        let canary = "refresh-token-canary";

        let error = refresh_openai_codex_oauth_with(&state, &tenant, true, |_| async move {
            Err(anyhow::anyhow!(
                "Codex OAuth refresh failed with HTTP 401: {canary}"
            ))
        })
        .await
        .expect_err("revoked token should fail refresh");
        assert_eq!(
            openai_codex_oauth_refresh_failure_code(&error),
            "credential_rejected"
        );

        let mut observed = Vec::new();
        while observed.len() < 2 {
            let event = tokio::time::timeout(std::time::Duration::from_secs(2), events.recv())
                .await
                .expect("OAuth event timeout")
                .expect("OAuth event");
            if matches!(
                event.event_type.as_str(),
                "provider.oauth.refresh.failed" | "provider.oauth.reauth_required"
            ) {
                let encoded = serde_json::to_string(&event.properties).expect("event json");
                assert!(!encoded.contains(canary));
                assert!(encoded.contains("credential_rejected"));
                observed.push(event.event_type);
            }
        }
        observed.sort();
        assert_eq!(
            observed,
            vec![
                "provider.oauth.reauth_required".to_string(),
                "provider.oauth.refresh.failed".to_string(),
            ]
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn transient_refresh_failure_does_not_force_reauthorization() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::explicit("org-transient", "workspace", None);
        save_scoped_codex_oauth(&state, &tenant, scoped_codex_oauth("transient", 1));
        let mut events = state.event_bus.subscribe();

        let error = refresh_openai_codex_oauth_with(&state, &tenant, true, |_| async {
            Err(anyhow::anyhow!("temporary OAuth endpoint timeout"))
        })
        .await
        .expect_err("transient refresh should fail");
        assert_eq!(
            openai_codex_oauth_refresh_failure_code(&error),
            "refresh_failed"
        );

        let failed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let event = events.recv().await.expect("OAuth event");
                if event.event_type == "provider.oauth.refresh.failed" {
                    break event;
                }
            }
        })
        .await
        .expect("refresh failure event");
        assert_eq!(
            failed.properties.get("failureCode").and_then(Value::as_str),
            Some("refresh_failed")
        );
        let unexpected_reauth =
            tokio::time::timeout(std::time::Duration::from_millis(100), events.recv())
                .await
                .ok()
                .and_then(Result::ok)
                .is_some_and(|event| event.event_type == "provider.oauth.reauth_required");
        assert!(!unexpected_reauth);
    }
}

fn provider_base_url(cfg: &Value, provider_id: &str) -> Option<String> {
    provider_config_value(cfg, provider_id)
        .and_then(|entry| entry.get("url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| provider_default_url(provider_id).map(str::to_string))
}

fn normalize_openai_catalog_base(input: &str) -> String {
    let mut base = input.trim().trim_end_matches('/').to_string();
    for suffix in ["/chat/completions", "/completions", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    while let Some(prefix) = base.strip_suffix("/v1") {
        if prefix.ends_with("/v1") {
            base = prefix.to_string();
            continue;
        }
        break;
    }
    if base.ends_with("/v1") {
        base
    } else {
        format!("{}/v1", base.trim_end_matches('/'))
    }
}

fn parse_openrouter_model_payload(body: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let data = body.get("data").and_then(|v| v.as_array())?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        let context = item
            .get("context_length")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                item.get("top_provider")
                    .and_then(|v| v.get("context_length"))
                    .and_then(|v| v.as_u64())
            })
            .map(|v| v as u32);

        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name,
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }
    (!out.is_empty()).then_some(out)
}

fn parse_openai_compatible_model_payload(
    body: &Value,
) -> Option<HashMap<String, WireProviderModel>> {
    let data = body.get("data").and_then(|v| v.as_array())?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        let context = item
            .get("context_window")
            .and_then(|v| v.as_u64())
            .or_else(|| item.get("context_length").and_then(|v| v.as_u64()))
            .map(|v| v as u32);
        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name,
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }
    (!out.is_empty()).then_some(out)
}

const PROVIDER_CATALOG_RESPONSE_MAX_BYTES: usize = 4 * 1024 * 1024;

async fn fetch_hardened_provider_response(
    url: &str,
    allow_standalone_private_endpoint: bool,
    configure: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
) -> Result<(reqwest::StatusCode, Vec<u8>), String> {
    let target = if allow_standalone_private_endpoint {
        crate::outbound_http::resolve_standalone_provider_url(url).await
    } else {
        crate::outbound_http::resolve_public_https_url(url).await
    }
    .map_err(|error| format!("Provider URL rejected: {error}"))?;
    let client = target
        .client(Duration::from_secs(20))
        .map_err(|error| format!("Failed to build provider HTTP client: {error}"))?;
    let response = configure(client.get(target.url().clone()))
        .send()
        .await
        .map_err(|error| format!("Provider request failed: {error}"))?;
    let status = response.status();
    let body = crate::outbound_http::read_response_body_limited(
        response,
        PROVIDER_CATALOG_RESPONSE_MAX_BYTES,
    )
    .await
    .map_err(|error| format!("Failed to read provider response: {error}"))?;
    Ok((status, body))
}

async fn fetch_openrouter_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let api_key = provider_config_api_key(
        cfg,
        "openrouter",
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    )
    .or_else(|| {
        allow_shared_auth_sources
            .then(|| std::env::var("OPENCODE_OPENROUTER_API_KEY").ok())
            .flatten()
    })
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());
    let Some(api_key) = api_key else {
        return Err(
            "OpenRouter requires an API key before live model discovery is available.".to_string(),
        );
    };

    let (status, body) = fetch_hardened_provider_response(
        "https://openrouter.ai/api/v1/models",
        false,
        |request| request.bearer_auth(api_key),
    )
        .await
        .map_err(|err| format!("Failed to fetch OpenRouter models: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "OpenRouter model catalog request failed with status {}",
            status
        ));
    }

    let body: Value = serde_json::from_slice(&body)
        .map_err(|err| format!("Failed to decode OpenRouter models: {err}"))?;
    parse_openrouter_model_payload(&body)
        .ok_or_else(|| "OpenRouter returned an empty or invalid model catalog.".to_string())
}

async fn fetch_openai_compatible_models(
    provider_id: &str,
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
    allow_standalone_private_endpoint: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let api_key = provider_config_api_key(
        cfg,
        provider_id,
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    );
    if api_key.is_none() && !provider_supports_optional_auth(provider_id) {
        return Err(format!(
            "{} requires an API key before live model discovery is available.",
            known_provider_name(provider_id).unwrap_or(provider_id)
        ));
    }
    let Some(base_url) = provider_base_url(cfg, provider_id) else {
        return Err("No provider base URL is configured for live model discovery.".to_string());
    };

    let url = format!("{}/models", normalize_openai_catalog_base(&base_url));
    let (status, body) =
        fetch_hardened_provider_response(&url, allow_standalone_private_endpoint, |request| {
            if let Some(api_key) = api_key {
                request.bearer_auth(api_key)
            } else {
                request
            }
        })
        .await
        .map_err(|err| format!("Failed to fetch model catalog from {provider_id}: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "{} model catalog request failed with status {}",
            provider_id, status
        ));
    }
    let body: Value = serde_json::from_slice(&body)
        .map_err(|err| format!("Failed to decode model catalog from {provider_id}: {err}"))?;
    parse_openai_compatible_model_payload(&body).ok_or_else(|| {
        format!("{provider_id} returned an empty or invalid OpenAI-compatible model catalog.")
    })
}

async fn fetch_openai_codex_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let Some(api_key) = provider_config_api_key(
        cfg,
        OPENAI_CODEX_PROVIDER_ID,
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    ) else {
        return Err(
            "OpenAI Codex requires a connected account before live model discovery is available."
                .to_string(),
        );
    };
    let base_url = provider_base_url(cfg, OPENAI_CODEX_PROVIDER_ID)
        .unwrap_or_else(|| OPENAI_CODEX_API_BASE_URL.to_string());
    let url = format!("{}/models", base_url.trim_end_matches('/'));
    let (status, body) =
        fetch_hardened_provider_response(&url, false, |request| request.bearer_auth(api_key))
        .await
        .map_err(|err| format!("Failed to fetch OpenAI Codex models: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "OpenAI Codex model catalog request failed with status {}",
            status
        ));
    }
    let body: Value = serde_json::from_slice(&body)
        .map_err(|err| format!("Failed to decode OpenAI Codex models: {err}"))?;
    parse_openai_compatible_model_payload(&body)
        .ok_or_else(|| "OpenAI Codex returned an empty or invalid model catalog.".to_string())
}

fn normalize_cohere_catalog_base(input: &str) -> String {
    let mut base = input.trim().trim_end_matches('/').to_string();
    for suffix in ["/v1", "/v2", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    format!("{}/v1", base.trim_end_matches('/'))
}

fn parse_anthropic_model_payload(body: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let data = body.get("data").and_then(|v| v.as_array())?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("display_name")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        out.insert(
            model_id.to_string(),
            WireProviderModel { name, limit: None },
        );
    }
    (!out.is_empty()).then_some(out)
}

async fn fetch_anthropic_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let Some(api_key) = provider_config_api_key(
        cfg,
        "anthropic",
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    ) else {
        return Err(
            "Anthropic requires an API key before live model discovery is available.".to_string(),
        );
    };
    let (status, body) = fetch_hardened_provider_response(
        "https://api.anthropic.com/v1/models",
        false,
        |request| {
            request
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
        },
    )
        .await
        .map_err(|err| format!("Failed to fetch Anthropic models: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "Anthropic model catalog request failed with status {}",
            status
        ));
    }
    let body: Value = serde_json::from_slice(&body)
        .map_err(|err| format!("Failed to decode Anthropic models: {err}"))?;
    parse_anthropic_model_payload(&body)
        .ok_or_else(|| "Anthropic returned an empty or invalid model catalog.".to_string())
}

fn parse_cohere_model_payload(body: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let data = body
        .get("models")
        .and_then(|v| v.as_array())
        .or_else(|| body.get("data").and_then(|v| v.as_array()))?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item
            .get("name")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("id").and_then(|v| v.as_str()))
        else {
            continue;
        };
        let context = item
            .get("context_length")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name: Some(model_id.to_string()),
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }
    (!out.is_empty()).then_some(out)
}

async fn fetch_cohere_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let Some(api_key) = provider_config_api_key(
        cfg,
        "cohere",
        runtime_auth,
        persisted_auth,
        allow_shared_auth_sources,
    ) else {
        return Err(
            "Cohere requires an API key before live model discovery is available.".to_string(),
        );
    };
    let base_url = provider_config_value(cfg, "cohere")
        .and_then(|entry| entry.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("https://api.cohere.com/v2");
    let url = format!("{}/models", normalize_cohere_catalog_base(base_url));
    let (status, body) = fetch_hardened_provider_response(
        &url,
        allow_shared_auth_sources,
        |request| request.bearer_auth(api_key),
    )
        .await
        .map_err(|err| format!("Failed to fetch Cohere models: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "Cohere model catalog request failed with status {}",
            status
        ));
    }
    let body: Value = serde_json::from_slice(&body)
        .map_err(|err| format!("Failed to decode Cohere models: {err}"))?;
    parse_cohere_model_payload(&body)
        .ok_or_else(|| "Cohere returned an empty or invalid model catalog.".to_string())
}

async fn fetch_remote_provider_models(
    provider_id: &str,
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
    allow_shared_auth_sources: bool,
    allow_standalone_private_endpoint: bool,
) -> ProviderCatalogFetchResult {
    match provider_id {
        "openai-codex" => {
            match fetch_openai_codex_models(
                cfg,
                runtime_auth,
                persisted_auth,
                allow_shared_auth_sources,
            )
            .await
            {
                Ok(models) => ProviderCatalogFetchResult::Remote { models },
                Err(message) => {
                    tracing::debug!(
                        "openai-codex catalog discovery fell back to static: {message}"
                    );
                    ProviderCatalogFetchResult::Static {
                        models: codex_starter_models(),
                    }
                }
            }
        }
        "openrouter" => match fetch_openrouter_models(
            cfg,
            runtime_auth,
            persisted_auth,
            allow_shared_auth_sources,
        )
        .await
        {
            Ok(models) => ProviderCatalogFetchResult::Remote { models },
            Err(message) => {
                if message.contains("requires an API key") {
                    ProviderCatalogFetchResult::Unavailable { message }
                } else {
                    tracing::debug!("openrouter catalog discovery failed: {message}");
                    ProviderCatalogFetchResult::Error { message }
                }
            }
        },
        "openai" | "llama_cpp" | "groq" | "mistral" | "together" => {
            match fetch_openai_compatible_models(
                provider_id,
                cfg,
                runtime_auth,
                persisted_auth,
                allow_shared_auth_sources,
                allow_standalone_private_endpoint,
            )
            .await
            {
                Ok(models) => ProviderCatalogFetchResult::Remote { models },
                Err(message) => {
                    if message.contains("requires an API key") {
                        ProviderCatalogFetchResult::Unavailable { message }
                    } else {
                        tracing::warn!("{provider_id} catalog discovery failed: {message}");
                        ProviderCatalogFetchResult::Error { message }
                    }
                }
            }
        }
        "anthropic" => match fetch_anthropic_models(
            cfg,
            runtime_auth,
            persisted_auth,
            allow_shared_auth_sources,
        )
        .await
        {
            Ok(models) => ProviderCatalogFetchResult::Remote { models },
            Err(message) => {
                if message.contains("requires an API key") {
                    ProviderCatalogFetchResult::Unavailable { message }
                } else {
                    tracing::warn!("anthropic catalog discovery failed: {message}");
                    ProviderCatalogFetchResult::Error { message }
                }
            }
        },
        "cohere" => {
            match fetch_cohere_models(cfg, runtime_auth, persisted_auth, allow_shared_auth_sources)
                .await
            {
                Ok(models) => ProviderCatalogFetchResult::Remote { models },
                Err(message) => {
                    if message.contains("requires an API key") {
                        ProviderCatalogFetchResult::Unavailable { message }
                    } else {
                        tracing::warn!("cohere catalog discovery failed: {message}");
                        ProviderCatalogFetchResult::Error { message }
                    }
                }
            }
        }
        "azure" | "vertex" | "bedrock" | "copilot" | "ollama" => {
            ProviderCatalogFetchResult::Unavailable {
                message: remote_catalog_support_message(provider_id),
            }
        }
        _ => ProviderCatalogFetchResult::Unavailable {
            message: "Live model discovery is unavailable. Enter a model ID manually.".to_string(),
        },
    }
}

fn provider_supports_optional_auth(provider_id: &str) -> bool {
    matches!(canonical_provider_id(provider_id).as_str(), "llama_cpp")
}
