//! Per-tenant inbound request rate limiting (TAN2-11).
//!
//! The runtime previously had no request rate limiting anywhere: a single
//! shared transport token gated everything with no 429 and no per-tenant quota,
//! so abuse control had to live entirely in an upstream gateway that isn't part
//! of this repo. This adds an in-process, per-tenant fixed-window limiter
//! enforced in the auth gate once the tenant context has been resolved.
//!
//! Distinct from `app::state::automation::rate_limit`, which throttles
//! *outbound* provider calls. This one throttles *inbound* HTTP requests.
//!
//! Disabled by default (limit 0) so single-tenant / self-hosted deployments are
//! unaffected until an operator sets `TANDEM_TENANT_RATE_LIMIT_PER_MIN`.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Prune the per-tenant window map once it grows past this many entries, so a
/// churn of short-lived tenant keys cannot grow it without bound.
const PRUNE_THRESHOLD: usize = 4096;

struct Window {
    start_ms: u64,
    count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateDecision {
    Allowed,
    Limited { retry_after_secs: u64 },
}

pub struct TenantRateLimiter {
    windows: Mutex<HashMap<String, Window>>,
    limit: u32,
    window_ms: u64,
}

impl TenantRateLimiter {
    pub fn new(limit: u32, window_ms: u64) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            limit,
            window_ms: window_ms.max(1),
        }
    }

    /// `limit == 0` disables the limiter entirely.
    pub fn is_enabled(&self) -> bool {
        self.limit > 0
    }

    /// Record a request for `key` at `now_ms` and decide whether it is allowed.
    pub fn check(&self, key: &str, now_ms: u64) -> RateDecision {
        if self.limit == 0 {
            return RateDecision::Allowed;
        }
        let mut windows = self.windows.lock().unwrap_or_else(|e| e.into_inner());
        if windows.len() > PRUNE_THRESHOLD {
            windows.retain(|_, w| now_ms.saturating_sub(w.start_ms) < self.window_ms);
        }
        let window = windows.entry(key.to_string()).or_insert(Window {
            start_ms: now_ms,
            count: 0,
        });
        if now_ms.saturating_sub(window.start_ms) >= self.window_ms {
            window.start_ms = now_ms;
            window.count = 0;
        }
        window.count = window.count.saturating_add(1);
        if window.count > self.limit {
            let elapsed = now_ms.saturating_sub(window.start_ms);
            let retry_after_ms = self.window_ms.saturating_sub(elapsed);
            RateDecision::Limited {
                retry_after_secs: retry_after_ms.div_ceil(1000).max(1),
            }
        } else {
            RateDecision::Allowed
        }
    }
}

fn resolve_limit_per_min() -> u32 {
    std::env::var("TANDEM_TENANT_RATE_LIMIT_PER_MIN")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

/// Process-global limiter, configured once from the environment. Rate limiting
/// is per-process, so a single global instance is correct.
pub fn global() -> &'static TenantRateLimiter {
    static LIMITER: OnceLock<TenantRateLimiter> = OnceLock::new();
    LIMITER.get_or_init(|| TenantRateLimiter::new(resolve_limit_per_min(), 60_000))
}

/// Tenant key for the limiter: org + workspace, unit-separated so distinct
/// tenants can never collide.
pub fn tenant_key(org_id: &str, workspace_id: &str) -> String {
    format!("{org_id}\u{1f}{workspace_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_limiter_always_allows() {
        let limiter = TenantRateLimiter::new(0, 60_000);
        assert!(!limiter.is_enabled());
        for i in 0..1000 {
            assert_eq!(limiter.check("t", i), RateDecision::Allowed);
        }
    }

    #[test]
    fn allows_up_to_limit_then_rejects_within_window() {
        let limiter = TenantRateLimiter::new(3, 60_000);
        assert_eq!(limiter.check("t", 0), RateDecision::Allowed);
        assert_eq!(limiter.check("t", 1), RateDecision::Allowed);
        assert_eq!(limiter.check("t", 2), RateDecision::Allowed);
        match limiter.check("t", 3) {
            RateDecision::Limited { retry_after_secs } => assert!(retry_after_secs >= 1),
            other => panic!("expected Limited, got {other:?}"),
        }
    }

    #[test]
    fn window_resets_after_it_elapses() {
        let limiter = TenantRateLimiter::new(2, 1_000);
        assert_eq!(limiter.check("t", 0), RateDecision::Allowed);
        assert_eq!(limiter.check("t", 100), RateDecision::Allowed);
        assert!(matches!(
            limiter.check("t", 200),
            RateDecision::Limited { .. }
        ));
        // A full window later, the counter resets.
        assert_eq!(limiter.check("t", 1_000), RateDecision::Allowed);
    }

    #[test]
    fn limits_are_per_tenant() {
        let limiter = TenantRateLimiter::new(1, 60_000);
        assert_eq!(limiter.check("tenant-a", 0), RateDecision::Allowed);
        // tenant-a is now at its limit, but tenant-b is independent.
        assert!(matches!(
            limiter.check("tenant-a", 1),
            RateDecision::Limited { .. }
        ));
        assert_eq!(limiter.check("tenant-b", 1), RateDecision::Allowed);
    }

    #[test]
    fn retry_after_reflects_remaining_window() {
        let limiter = TenantRateLimiter::new(1, 10_000);
        assert_eq!(limiter.check("t", 0), RateDecision::Allowed);
        match limiter.check("t", 3_000) {
            // ~7s remaining in the 10s window.
            RateDecision::Limited { retry_after_secs } => assert_eq!(retry_after_secs, 7),
            other => panic!("expected Limited, got {other:?}"),
        }
    }

    #[test]
    fn tenant_key_separates_org_and_workspace() {
        assert_ne!(tenant_key("a", "bc"), tenant_key("ab", "c"));
    }
}
