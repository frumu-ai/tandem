// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Default-on pre-auth limiter for public automation webhook capability URLs.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, OnceLock};

const WINDOW_MS: u64 = 60_000;
const DEFAULT_CAPABILITY_LIMIT: u32 = 120;
const DEFAULT_NETWORK_LIMIT: u32 = 30;
const PRUNE_THRESHOLD: usize = 8_192;
const MAX_TRACKED_WINDOWS: usize = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RateDecision {
    Allowed,
    Limited { retry_after_secs: u64 },
}

#[derive(Clone, Copy)]
struct Limits {
    capability: u32,
    network: u32,
}

struct Window {
    start_ms: u64,
    count: u32,
}

pub(super) struct WebhookRateLimiter {
    windows: Mutex<HashMap<String, Window>>,
    limits: Limits,
    window_ms: u64,
}

impl WebhookRateLimiter {
    #[cfg(test)]
    fn new(capability: u32, network: u32, window_ms: u64) -> Self {
        Self::with_limits(
            Limits {
                capability,
                network,
            },
            window_ms,
        )
    }

    fn with_limits(limits: Limits, window_ms: u64) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            limits,
            window_ms: window_ms.max(1),
        }
    }

    pub(super) fn check(
        &self,
        public_path_token: &str,
        peer: Option<SocketAddr>,
        now_ms: u64,
    ) -> RateDecision {
        let capability = crate::sha256_hex(&[public_path_token]);
        let network = coarse_network(peer);
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        prune_windows(&mut windows, now_ms, self.window_ms);

        let capability_retry = check_window(
            &mut windows,
            format!("cap:{capability}"),
            self.limits.capability,
            now_ms,
            self.window_ms,
        );
        let network_retry = check_window(
            &mut windows,
            format!("net:{capability}:{network}"),
            self.limits.network,
            now_ms,
            self.window_ms,
        );
        match capability_retry.max(network_retry) {
            0 => RateDecision::Allowed,
            retry_after_secs => RateDecision::Limited { retry_after_secs },
        }
    }
}

fn check_window(
    windows: &mut HashMap<String, Window>,
    key: String,
    limit: u32,
    now_ms: u64,
    window_ms: u64,
) -> u64 {
    if limit == 0 {
        return 0;
    }
    let window = windows.entry(key).or_insert(Window {
        start_ms: now_ms,
        count: 0,
    });
    if now_ms.saturating_sub(window.start_ms) >= window_ms {
        window.start_ms = now_ms;
        window.count = 0;
    }
    window.count = window.count.saturating_add(1);
    if window.count <= limit {
        return 0;
    }
    window_ms
        .saturating_sub(now_ms.saturating_sub(window.start_ms))
        .div_ceil(1000)
        .max(1)
}

fn prune_windows(windows: &mut HashMap<String, Window>, now_ms: u64, window_ms: u64) {
    if windows.len() <= PRUNE_THRESHOLD {
        return;
    }
    windows.retain(|_, window| now_ms.saturating_sub(window.start_ms) < window_ms);
    while windows.len() >= MAX_TRACKED_WINDOWS {
        let Some(oldest) = windows
            .iter()
            .min_by_key(|(_, window)| window.start_ms)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        windows.remove(&oldest);
    }
}

fn coarse_network(peer: Option<SocketAddr>) -> String {
    match peer.map(|address| address.ip()) {
        Some(IpAddr::V4(address)) => {
            let [a, b, c, _] = address.octets();
            format!("v4:{a}.{b}.{c}.0/24")
        }
        Some(IpAddr::V6(address)) => {
            let segments = address.segments();
            format!(
                "v6:{:x}:{:x}:{:x}:{:x}::/64",
                segments[0], segments[1], segments[2], segments[3]
            )
        }
        None => "unknown".to_string(),
    }
}

fn env_limit(name: &str, fallback: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(fallback)
}

pub(super) fn global() -> &'static WebhookRateLimiter {
    static LIMITER: OnceLock<WebhookRateLimiter> = OnceLock::new();
    LIMITER.get_or_init(|| {
        WebhookRateLimiter::with_limits(
            Limits {
                capability: env_limit(
                    "TANDEM_PUBLIC_WEBHOOK_RATE_LIMIT_PER_MIN",
                    DEFAULT_CAPABILITY_LIMIT,
                ),
                network: env_limit(
                    "TANDEM_PUBLIC_WEBHOOK_NETWORK_RATE_LIMIT_PER_MIN",
                    DEFAULT_NETWORK_LIMIT,
                ),
            },
            WINDOW_MS,
        )
    })
}

pub(super) fn public_automation_webhook_token(path: &str) -> Option<String> {
    let trimmed = path
        .strip_prefix("/api/engine")
        .filter(|suffix| suffix.starts_with('/'))
        .unwrap_or(path);
    let parts = trimmed
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let token = match parts.as_slice() {
        ["webhooks", "automations", token] | ["webhooks", "automations", token, _] => *token,
        _ => return None,
    };
    let decoded = urlencoding::decode(token).ok()?.into_owned();
    (!decoded.is_empty() && decoded.len() <= 256 && decoded.is_ascii() && !decoded.contains('/'))
        .then_some(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_each_capability_and_network_before_auth() {
        let limiter = WebhookRateLimiter::new(3, 2, 60_000);
        let peer = Some("203.0.113.4:443".parse().unwrap());
        assert_eq!(limiter.check("whpub-a", peer, 0), RateDecision::Allowed);
        assert_eq!(limiter.check("whpub-a", peer, 1), RateDecision::Allowed);
        assert!(matches!(
            limiter.check("whpub-a", peer, 2),
            RateDecision::Limited { .. }
        ));
        assert_eq!(
            limiter.check("whpub-b", peer, 2),
            RateDecision::Allowed,
            "a distinct capability has an independent network bucket"
        );
    }

    #[test]
    fn parses_only_exact_public_webhook_shapes() {
        assert_eq!(
            public_automation_webhook_token("/webhooks/automations/whpub-a"),
            Some("whpub-a".to_string())
        );
        assert_eq!(
            public_automation_webhook_token("/api/engine/webhooks/automations/whpub-a/whsetup-a"),
            Some("whpub-a".to_string())
        );
        assert_eq!(
            public_automation_webhook_token("/webhooks/automations/%77hpub-a"),
            Some("whpub-a".to_string()),
            "percent-encoded aliases must share the canonical capability bucket"
        );
        assert_eq!(
            public_automation_webhook_token("/webhooks/automations/whpub-a/extra/path"),
            None
        );
        assert_eq!(
            public_automation_webhook_token("/webhooks/automations/%2Fetc"),
            None
        );
    }
}
