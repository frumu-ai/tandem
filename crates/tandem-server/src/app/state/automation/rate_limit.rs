use crate::util::time::now_ms;
use std::collections::HashMap;

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
        let status = self.providers.entry(provider.to_string()).or_default();
        status.is_throttled = true;
        status.throttled_until_ms = Some(now_ms() + retry_after_ms);
    }

    pub fn is_provider_throttled(&self, provider: &str) -> bool {
        if let Some(status) = self.providers.get(provider) {
            if status.is_throttled {
                if let Some(until) = status.throttled_until_ms {
                    return now_ms() < until;
                }
            }
        }
        false
    }

    pub fn clear_throttle(&mut self, provider: &str) {
        if let Some(status) = self.providers.get_mut(provider) {
            status.is_throttled = false;
            status.throttled_until_ms = None;
        }
    }
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
}
