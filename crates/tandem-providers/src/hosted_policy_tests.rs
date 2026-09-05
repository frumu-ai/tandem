#[cfg(test)]
mod hosted_policy_tests {
    use super::*;

    #[tokio::test]
    async fn hosted_policy_revocation_during_auth_recovery_prevents_provider_retry() {
        let registry = ProviderRegistry::new(cfg(&[], None, false));
        let tenant = TenantContext::explicit("org-hosted", "workspace-hosted", None);
        let attempts = Arc::new(AtomicUsize::new(0));
        registry
            .replace_for_test(
                vec![Arc::new(CapturingCodexProvider {
                    attempts: attempts.clone(),
                    seen_auth: Arc::new(Mutex::new(Vec::new())),
                    fail_auth_attempts: 1,
                })],
                Some("openai-codex".into()),
            )
            .await;
        let revoked = Arc::new(AtomicBool::new(false));
        let recovery = ProviderAuthRecovery::new({
            let revoked = revoked.clone();
            move |_| {
                let revoked = revoked.clone();
                async move {
                    revoked.store(true, Ordering::SeqCst);
                    Ok(true)
                }
            }
        });
        let authority = ProviderDispatchAuthority::new(move || {
            let revoked = revoked.clone();
            async move {
                anyhow::ensure!(!revoked.load(Ordering::SeqCst), "hosted membership revoked");
                Ok(())
            }
        });
        let error = authority
            .scope(registry.scope_tenant_provider_auth_with_recovery(
                tenant,
                recovery,
                false,
                registry.complete_for_provider(Some("openai-codex"), "synthetic request", None),
            ))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("revoked"));
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            1,
            "revoked request must not retry after credential refresh"
        );
    }

    #[tokio::test]
    async fn hosted_policy_dispatch_authority_is_isolated_between_concurrent_requests() {
        let registry = ProviderRegistry::new(cfg(&[], None, false));
        let attempts = Arc::new(AtomicUsize::new(0));
        registry
            .replace_for_test(
                vec![Arc::new(CapturingCodexProvider {
                    attempts: attempts.clone(),
                    seen_auth: Arc::new(Mutex::new(Vec::new())),
                    fail_auth_attempts: 0,
                })],
                Some("openai-codex".into()),
            )
            .await;
        let denied =
            ProviderDispatchAuthority::new(|| async { anyhow::bail!("membership revoked") });
        let allowed = ProviderDispatchAuthority::new(|| async { Ok(()) });
        let (denied, allowed) = tokio::join!(
            denied.scope(registry.complete_for_provider(Some("openai-codex"), "denied", None)),
            allowed.scope(registry.complete_for_provider(Some("openai-codex"), "allowed", None)),
        );
        assert!(denied.unwrap_err().to_string().contains("revoked"));
        assert_eq!(allowed.unwrap(), "allowed");
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }
}
