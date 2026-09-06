#[cfg(test)]
mod runtime_binding_tests {
    use super::provider_attempt_tests::{compatible, responses};
    use super::*;

    fn tenant(name: &str) -> TenantContext {
        TenantContext::explicit(name, "workspace", Some("deployment".into()))
    }

    fn configured(url: &str, key: Option<&str>) -> AppConfig {
        AppConfig {
            providers: [(
                "llama_cpp".into(),
                ProviderConfig {
                    url: Some(url.into()),
                    api_key: key.map(str::to_string),
                    default_model: Some("synthetic-model".into()),
                },
            )]
            .into(),
            default_provider: Some("llama_cpp".into()),
        }
    }

    #[tokio::test]
    async fn runtime_binding_reads_current_route_credentials_and_model_without_network() {
        let registry = ProviderRegistry::new(configured(
            "https://one.invalid/v1",
            Some("synthetic-host-secret"),
        ));
        let scope = tenant("org-a");
        let first = registry
            .runtime_binding_for_tenant(&scope, "llama_cpp", "synthetic-model")
            .await
            .unwrap();
        assert_eq!(
            first.credential_source,
            ProviderCredentialSource::HostService
        );
        let encoded = serde_json::to_string(&first).unwrap();
        assert!(!encoded.contains("synthetic-host-secret"));
        assert!(!encoded.contains("one.invalid"));
        registry
            .reload(configured(
                "https://two.invalid/v1",
                Some("synthetic-host-secret"),
            ))
            .await;
        let route = registry
            .runtime_binding_for_tenant(&scope, "llama_cpp", "synthetic-model")
            .await
            .unwrap();
        assert_ne!(first.endpoint_sha256, route.endpoint_sha256);
        assert_eq!(first.credential_sha256, route.credential_sha256);
        registry
            .reload(configured(
                "https://two.invalid/v1",
                Some("synthetic-replacement"),
            ))
            .await;
        let key = registry
            .runtime_binding_for_tenant(&scope, "llama_cpp", "synthetic-model")
            .await
            .unwrap();
        assert_ne!(route.credential_sha256, key.credential_sha256);
        registry
            .reload(configured("https://two.invalid/v1", None))
            .await;
        let no_key = registry
            .runtime_binding_for_tenant(&scope, "llama_cpp", "synthetic-model")
            .await
            .unwrap();
        assert_eq!(
            no_key.credential_source,
            ProviderCredentialSource::Unauthenticated
        );
        assert!(registry
            .runtime_binding_for_tenant(&scope, "llama_cpp", "missing-model")
            .await
            .is_err());
        assert!(registry
            .runtime_binding_for_tenant(&scope, "", "synthetic-model")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn runtime_binding_scopes_codex_credentials_and_never_inherits_for_missing_tenant() {
        let registry = ProviderRegistry::new(AppConfig::default());
        let mut provider = responses("https://synthetic.invalid".into());
        provider.api_key = Some("synthetic-global-token".into());
        provider.models = vec![ModelInfo {
            id: "synthetic-model".into(),
            provider_id: "openai-codex".into(),
            display_name: "Synthetic".into(),
            context_window: 100,
        }];
        registry
            .replace_for_test(vec![Arc::new(provider)], Some("openai-codex".into()))
            .await;
        let a = tenant("org-a");
        let b = tenant("org-b");
        assert!(registry
            .runtime_binding_for_tenant(&a, "openai-codex", "synthetic-model")
            .await
            .unwrap_err()
            .to_string()
            .contains("credential missing"));
        registry
            .set_tenant_provider_bearer_token(&a, "openai-codex", "synthetic-a".into())
            .await;
        registry
            .set_tenant_provider_bearer_token(&b, "openai-codex", "synthetic-b".into())
            .await;
        let (a_binding, b_binding) = tokio::join!(
            registry.runtime_binding_for_tenant(&a, "openai-codex", "synthetic-model"),
            registry.runtime_binding_for_tenant(&b, "openai-codex", "synthetic-model")
        );
        let a_binding = a_binding.unwrap();
        let b_binding = b_binding.unwrap();
        assert_eq!(
            a_binding.credential_source,
            ProviderCredentialSource::TenantBearer
        );
        assert_ne!(a_binding.credential_sha256, b_binding.credential_sha256);
        assert_eq!(a_binding.endpoint_sha256, b_binding.endpoint_sha256);
        registry
            .clear_tenant_provider_bearer_token(&a, "openai-codex")
            .await;
        assert!(registry
            .runtime_binding_for_tenant(&a, "openai-codex", "synthetic-model")
            .await
            .is_err());
        assert_eq!(
            b_binding,
            registry
                .runtime_binding_for_tenant(&b, "openai-codex", "synthetic-model")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn runtime_binding_rejects_unknown_and_ambiguous_adapters() {
        let registry = ProviderRegistry::new(AppConfig::default());
        assert!(registry
            .runtime_binding_for_tenant(&tenant("a"), "local", "synthetic-model")
            .await
            .is_err());
        registry
            .replace_for_test(
                vec![
                    Arc::new(compatible("http://127.0.0.1:1".into())),
                    Arc::new(compatible("http://127.0.0.1:2".into())),
                ],
                None,
            )
            .await;
        assert!(registry
            .runtime_binding_for_tenant(&tenant("a"), "ollama", "synthetic-model")
            .await
            .unwrap_err()
            .to_string()
            .contains("ambiguous"));
    }

    #[tokio::test]
    async fn runtime_binding_matches_three_actual_adapters_before_network() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let mut compatible = compatible(endpoint.clone());
        compatible.api_key = Some("synthetic-host-key".into());
        let mut responses = responses(endpoint.clone());
        responses.models = vec![ModelInfo {
            id: "synthetic-model".into(),
            provider_id: "openai-codex".into(),
            display_name: "Synthetic".into(),
            context_window: 100,
        }];
        let providers: Vec<Arc<dyn Provider>> = vec![
            Arc::new(compatible),
            Arc::new(responses),
            Arc::new(AnthropicProvider {
                api_key: Some("synthetic-anthropic".into()),
                default_model: "synthetic-model".into(),
                client: Client::builder()
                    .proxy(reqwest::Proxy::all(&endpoint).unwrap())
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(Duration::from_secs(3))
                    .build()
                    .unwrap(),
            }),
            Arc::new(CohereProvider {
                api_key: Some("synthetic-cohere".into()),
                base_url: "https://cohere.invalid/v2".into(),
                default_model: "synthetic-model".into(),
            }),
        ];
        let registry = ProviderRegistry::new(AppConfig::default());
        registry.replace_for_test(providers, None).await;
        let scope = tenant("org-a");
        registry
            .set_tenant_provider_bearer_token(&scope, "openai-codex", "synthetic-tenant-key".into())
            .await;
        let cohere = registry
            .runtime_binding_for_tenant(&scope, "cohere", "synthetic-model")
            .await
            .unwrap();
        assert_eq!(cohere.protocol, ProviderProtocol::Cohere);
        assert_eq!(
            cohere.credential_source,
            ProviderCredentialSource::HostService
        );
        for id in ["ollama", "openai-codex", "anthropic"] {
            for streaming in [false, true] {
                let binding = registry
                    .runtime_binding_for_tenant(&scope, id, "synthetic-model")
                    .await
                    .unwrap();
                let count = Arc::new(AtomicUsize::new(0));
                let observed = count.clone();
                let policy = ProviderAttemptPolicy::new(10, 4096, move |attempt| {
                    assert!(
                        binding.matches_attempt(&attempt),
                        "snapshot must describe final request bytes"
                    );
                    observed.fetch_add(1, Ordering::SeqCst);
                    async { anyhow::bail!("verified snapshot; stopped before transport") }
                })
                .unwrap();
                let result = registry
                    .scope_tenant_provider_auth_with_recovery(
                        scope.clone(),
                        ProviderAuthRecovery::new(|_| async { Ok(false) }),
                        true,
                        policy.scope(async {
                            if streaming {
                                registry
                                    .stream_for_provider(
                                        Some(id),
                                        Some("synthetic-model"),
                                        vec![ChatMessage {
                                            role: "user".into(),
                                            content: "synthetic".into(),
                                            attachments: Vec::new(),
                                        }],
                                        ToolMode::None,
                                        None,
                                        SamplingParams::default(),
                                        CancellationToken::new(),
                                    )
                                    .await
                                    .map(|_| ())
                            } else {
                                registry
                                    .complete_for_provider(
                                        Some(id),
                                        "synthetic",
                                        Some("synthetic-model"),
                                    )
                                    .await
                                    .map(|_| ())
                            }
                        }),
                    )
                    .await;
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("verified snapshot"));
                assert_eq!(count.load(Ordering::SeqCst), 1);
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn runtime_binding_uses_concrete_codex_route_instead_of_ignored_url_override() {
        let config = |url: &str| AppConfig {
            providers: [(
                "openai-codex".into(),
                ProviderConfig {
                    url: Some(url.into()),
                    api_key: None,
                    default_model: None,
                },
            )]
            .into(),
            default_provider: Some("openai-codex".into()),
        };
        let registry = ProviderRegistry::new(config("https://ignored-one.invalid"));
        let scope = tenant("org-a");
        registry
            .set_tenant_provider_bearer_token(&scope, "openai-codex", "synthetic-tenant".into())
            .await;
        let model = registry
            .list()
            .await
            .into_iter()
            .find(|provider| provider.id == "openai-codex")
            .unwrap()
            .models[0]
            .id
            .clone();
        let before = registry
            .runtime_binding_for_tenant(&scope, "openai-codex", &model)
            .await
            .unwrap();
        registry.reload(config("https://ignored-two.invalid")).await;
        let after = registry
            .runtime_binding_for_tenant(&scope, "openai-codex", &model)
            .await
            .unwrap();
        assert_eq!(before, after);
    }
}
