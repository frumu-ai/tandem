use crate::*;
use tempfile::tempdir;

fn tenant() -> TenantContext {
    TenantContext::explicit(
        uuid::Uuid::new_v4().to_string(),
        "workspace",
        Some("actor".into()),
    )
}

fn configured(provider: &str, key: Option<&str>, model: &str) -> AppConfig {
    AppConfig {
        providers: [(
            provider.into(),
            ProviderConfig {
                url: Some("https://synthetic.invalid/v1".into()),
                api_key: key.map(str::to_string),
                default_model: Some(model.into()),
            },
        )]
        .into(),
        default_provider: Some(provider.into()),
    }
}

fn save_host(dir: &std::path::Path, provider: &str, key: &str) {
    set_provider_auth_for_tenant_in_dir(dir, &TenantContext::local_implicit(), provider, key)
        .unwrap();
}

async fn binding(
    registry: &ProviderRegistry,
    dir: &std::path::Path,
    tenant: &TenantContext,
) -> anyhow::Result<VersionedProviderRuntimeBinding> {
    registry
        .versioned_runtime_binding_for_tenant_in_dir(
            dir,
            tenant,
            "llama_cpp",
            "synthetic-model",
            ProviderCredentialKind::ApiKey,
            ProviderCredentialLocation::HostService,
        )
        .await
}

#[tokio::test]
async fn current_revision_cannot_approve_stale_loaded_material_or_restore_an_old_approval() {
    let dir = tempdir().unwrap();
    let tenant = tenant();
    let registry = ProviderRegistry::new(configured(
        "llama_cpp",
        Some("synthetic-a"),
        "synthetic-model",
    ));
    assert!(
        binding(&registry, dir.path(), &tenant).await.is_err(),
        "untracked config key"
    );
    save_host(dir.path(), "llama_cpp", "synthetic-a");
    let first = binding(&registry, dir.path(), &tenant).await.unwrap();
    save_host(dir.path(), "llama_cpp", "synthetic-b");
    assert!(
        binding(&registry, dir.path(), &tenant).await.is_err(),
        "new revision cannot approve old runtime key"
    );
    registry
        .reload(configured(
            "llama_cpp",
            Some("synthetic-b"),
            "synthetic-model",
        ))
        .await;
    let second = binding(&registry, dir.path(), &tenant).await.unwrap();
    assert_ne!(
        first.revision.authorization_revision,
        second.revision.authorization_revision
    );
    save_host(dir.path(), "llama_cpp", "synthetic-a");
    registry
        .reload(configured(
            "llama_cpp",
            Some("synthetic-a"),
            "synthetic-model",
        ))
        .await;
    let restored = binding(&registry, dir.path(), &tenant).await.unwrap();
    assert_eq!(
        restored.runtime.credential_sha256,
        first.runtime.credential_sha256
    );
    assert_ne!(
        restored.revision.authorization_revision,
        first.revision.authorization_revision
    );
    let public = format!("{restored:?} {}", serde_json::to_string(&restored).unwrap());
    assert!(!public.contains("synthetic-a"));
    assert!(!public.contains("synthetic.invalid"));
    delete_provider_auth_for_tenant_in_dir(
        dir.path(),
        &TenantContext::local_implicit(),
        "llama_cpp",
    )
    .unwrap();
    assert!(
        binding(&registry, dir.path(), &tenant).await.is_err(),
        "disconnect cannot reuse still-loaded bearer"
    );
}

#[tokio::test]
async fn tenant_account_lookup_requires_both_selected_scope_and_its_own_persisted_material() {
    let dir = tempdir().unwrap();
    let alpha = tenant();
    let beta = tenant();
    let registry = ProviderRegistry::new(configured(
        "openai-codex",
        Some("synthetic-shared"),
        "gpt-5.6-sol",
    ));
    save_host(dir.path(), "openai-codex", "synthetic-shared");
    set_provider_auth_for_tenant_in_dir(dir.path(), &alpha, "openai-codex", "synthetic-shared")
        .unwrap();
    let lookup = |scope: TenantContext| {
        let registry = registry.clone();
        let path = dir.path().to_path_buf();
        async move {
            registry
                .versioned_runtime_binding_for_tenant_in_dir(
                    &path,
                    &scope,
                    "openai-codex",
                    "gpt-5.6-sol",
                    ProviderCredentialKind::ApiKey,
                    ProviderCredentialLocation::TenantService,
                )
                .await
        }
    };
    assert!(
        lookup(alpha.clone()).await.is_err(),
        "global credentials do not authorize an unloaded tenant account"
    );
    registry
        .set_tenant_provider_bearer_token(&alpha, "openai-codex", "synthetic-shared".into())
        .await;
    lookup(alpha.clone()).await.unwrap();
    assert!(
        registry
            .versioned_runtime_binding_for_tenant_in_dir(
                dir.path(),
                &alpha,
                "openai-codex",
                "gpt-5.6-sol",
                ProviderCredentialKind::ApiKey,
                ProviderCredentialLocation::HostService,
            )
            .await
            .is_err(),
        "tenant bearer cannot be relabeled host service"
    );
    registry
        .set_tenant_provider_bearer_token(&beta, "openai-codex", "synthetic-shared".into())
        .await;
    assert!(
        lookup(beta.clone()).await.is_err(),
        "other tenant's index is not proof"
    );
    set_provider_auth_for_tenant_in_dir(dir.path(), &beta, "openai-codex", "synthetic-beta")
        .unwrap();
    assert!(
        lookup(beta.clone()).await.is_err(),
        "other tenant's matching loaded key does not match selected persisted material"
    );
    registry
        .set_tenant_provider_bearer_token(&beta, "openai-codex", "synthetic-beta".into())
        .await;
    lookup(beta).await.unwrap();
    assert!(lookup(TenantContext::local_implicit()).await.is_err());
}

fn oauth(label: &str) -> OAuthProviderCredential {
    OAuthProviderCredential {
        provider_id: "openai-codex".into(),
        access_token: format!("access-{label}"),
        refresh_token: format!("refresh-{label}"),
        expires_at_ms: 2_000_000_000_000,
        account_id: Some("synthetic-account".into()),
        email: None,
        display_name: None,
        managed_by: "tandem".into(),
        api_key: Some(format!("runtime-{label}")),
    }
}

#[tokio::test]
async fn oauth_binding_uses_runtime_api_key_and_preserves_only_same_account_refresh_authorization()
{
    let dir = tempdir().unwrap();
    let tenant = tenant();
    let registry = ProviderRegistry::new(configured("openai-codex", None, "gpt-5.6-sol"));
    let old = oauth("old");
    set_provider_oauth_credential_for_tenant_in_dir(
        dir.path(),
        &tenant,
        "openai-codex",
        old.clone(),
    )
    .unwrap();
    let lookup = || {
        registry.versioned_runtime_binding_for_tenant_in_dir(
            dir.path(),
            &tenant,
            "openai-codex",
            "gpt-5.6-sol",
            ProviderCredentialKind::Credential,
            ProviderCredentialLocation::TenantService,
        )
    };
    registry
        .set_tenant_provider_bearer_token(&tenant, "openai-codex", old.access_token.clone())
        .await;
    assert!(
        lookup().await.is_err(),
        "access token cannot substitute for selected runtime API key"
    );
    registry
        .set_tenant_provider_bearer_token(&tenant, "openai-codex", old.api_key.clone().unwrap())
        .await;
    let before = lookup().await.unwrap();
    let refreshed = oauth("refreshed");
    assert!(
        refresh_provider_oauth_credential_for_tenant_in_dir_serialized(
            dir.path(),
            &tenant,
            "openai-codex",
            &old,
            refreshed.clone(),
        )
        .await
        .unwrap()
    );
    assert!(
        lookup().await.is_err(),
        "persisted refresh must not approve old loaded token"
    );
    registry
        .set_tenant_provider_bearer_token(
            &tenant,
            "openai-codex",
            refreshed.api_key.clone().unwrap(),
        )
        .await;
    let after = lookup().await.unwrap();
    assert_eq!(
        after.revision.authorization_revision,
        before.revision.authorization_revision
    );
    assert_ne!(
        after.revision.material_revision,
        before.revision.material_revision
    );
    assert_ne!(
        after.runtime.credential_sha256,
        before.runtime.credential_sha256
    );
    set_provider_oauth_credential_for_tenant_in_dir(dir.path(), &tenant, "openai-codex", refreshed)
        .unwrap();
    assert_ne!(
        lookup().await.unwrap().revision.authorization_revision,
        before.revision.authorization_revision
    );
}

#[tokio::test]
async fn host_key_header_fingerprints_match_compatible_anthropic_and_cohere_adapters() {
    let dir = tempdir().unwrap();
    let tenant = tenant();
    for (provider, protocol) in [
        ("llama_cpp", ProviderProtocol::ChatCompletions),
        ("anthropic", ProviderProtocol::Anthropic),
        ("cohere", ProviderProtocol::Cohere),
    ] {
        let registry = ProviderRegistry::new(configured(
            provider,
            Some("synthetic-header-key"),
            "synthetic-model",
        ));
        save_host(dir.path(), provider, "synthetic-header-key");
        let binding = registry
            .versioned_runtime_binding_for_tenant_in_dir(
                dir.path(),
                &tenant,
                provider,
                "synthetic-model",
                ProviderCredentialKind::ApiKey,
                ProviderCredentialLocation::HostService,
            )
            .await
            .unwrap();
        assert_eq!(binding.runtime.protocol, protocol);
    }
}
