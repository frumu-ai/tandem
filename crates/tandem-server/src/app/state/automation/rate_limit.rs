use crate::util::time::now_ms;
use std::collections::HashMap;
use tandem_types::TenantContext;

#[derive(Debug, Default, Clone)]
pub struct ProviderRateLimitStatus {
    pub active_requests: usize,
    pub is_throttled: bool,
    pub throttled_until_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub struct RateLimitManager {
    providers: HashMap<String, ProviderRateLimitStatus>,
}

impl RateLimitManager {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn record_throttle(&mut self, provider: &str, retry_after_ms: u64) {
        self.record_throttle_for_tenant(&TenantContext::local_implicit(), provider, retry_after_ms);
    }

    pub fn record_throttle_for_tenant(
        &mut self,
        tenant_context: &TenantContext,
        provider: &str,
        retry_after_ms: u64,
    ) {
        let status = self
            .providers
            .entry(provider_rate_limit_key(tenant_context, provider))
            .or_default();
        status.is_throttled = true;
        status.throttled_until_ms = Some(now_ms() + retry_after_ms);
    }

    pub fn is_provider_throttled(&self, provider: &str) -> bool {
        self.is_provider_throttled_for_tenant(&TenantContext::local_implicit(), provider)
    }

    pub fn is_provider_throttled_for_tenant(
        &self,
        tenant_context: &TenantContext,
        provider: &str,
    ) -> bool {
        if let Some(status) = self
            .providers
            .get(&provider_rate_limit_key(tenant_context, provider))
        {
            if status.is_throttled {
                if let Some(until) = status.throttled_until_ms {
                    return now_ms() < until;
                }
            }
        }
        false
    }

    pub fn clear_throttle(&mut self, provider: &str) {
        self.clear_throttle_for_tenant(&TenantContext::local_implicit(), provider);
    }

    pub fn clear_throttle_for_tenant(&mut self, tenant_context: &TenantContext, provider: &str) {
        if let Some(status) = self
            .providers
            .get_mut(&provider_rate_limit_key(tenant_context, provider))
        {
            status.is_throttled = false;
            status.throttled_until_ms = None;
        }
    }
}

fn provider_rate_limit_key(tenant_context: &TenantContext, provider: &str) -> String {
    let provider = provider.trim().to_ascii_lowercase();
    if tenant_context.is_local_implicit() {
        return provider;
    }
    format!(
        "{}:{}:{}:{}",
        tenant_context.org_id.trim().to_ascii_lowercase(),
        tenant_context.workspace_id.trim().to_ascii_lowercase(),
        tenant_context
            .deployment_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase(),
        provider
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_throttle_expiry_is_not_permanent_throttle() {
        let mut manager = RateLimitManager::new();
        manager.providers.insert(
            "provider-a".to_string(),
            ProviderRateLimitStatus {
                active_requests: 0,
                is_throttled: true,
                throttled_until_ms: None,
            },
        );

        assert!(!manager.is_provider_throttled("provider-a"));
    }

    #[test]
    fn expired_throttle_is_not_active() {
        let mut manager = RateLimitManager::new();
        manager.providers.insert(
            "provider-a".to_string(),
            ProviderRateLimitStatus {
                active_requests: 0,
                is_throttled: true,
                throttled_until_ms: Some(now_ms().saturating_sub(1)),
            },
        );

        assert!(!manager.is_provider_throttled("provider-a"));
    }

    #[test]
    fn active_throttle_remains_throttled() {
        let mut manager = RateLimitManager::new();
        manager.providers.insert(
            "provider-a".to_string(),
            ProviderRateLimitStatus {
                active_requests: 0,
                is_throttled: true,
                throttled_until_ms: Some(now_ms().saturating_add(60_000)),
            },
        );

        assert!(manager.is_provider_throttled("provider-a"));
        let status = manager
            .providers
            .get("provider-a")
            .expect("provider status");
        assert!(status.is_throttled);
        assert!(status.throttled_until_ms.is_some());
    }

    #[test]
    fn provider_throttles_are_tenant_scoped() {
        let mut manager = RateLimitManager::new();
        let tenant_a = TenantContext::explicit("org-a", "workspace", Some("user-a".to_string()));
        let tenant_b = TenantContext::explicit("org-b", "workspace", Some("user-b".to_string()));

        manager.record_throttle_for_tenant(&tenant_a, "openai", 60_000);

        assert!(manager.is_provider_throttled_for_tenant(&tenant_a, "openai"));
        assert!(!manager.is_provider_throttled_for_tenant(&tenant_b, "openai"));
        assert!(!manager.is_provider_throttled("openai"));
    }
}
