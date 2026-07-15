// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::Value;
use tandem_channels::config::ChannelSecurityProfile;
use tandem_types::RequestPrincipal;
use tokio::sync::RwLock;

const DEFAULT_PROMPT_LIMIT_PER_MINUTE: u32 = 10;
const DEFAULT_DECISION_LIMIT_PER_MINUTE: u32 = 30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelRateLimitKey {
    pub channel: String,
    pub user_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelRateLimitKind {
    Prompt,
    Decision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelRateLimitDecision {
    pub allowed: bool,
    pub retry_after_secs: u64,
}

#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: u32, now: Instant) -> Self {
        Self {
            tokens: capacity as f64,
            last_refill: now,
        }
    }

    fn check(
        &mut self,
        capacity: u32,
        refill_per_sec: f64,
        now: Instant,
    ) -> ChannelRateLimitDecision {
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * refill_per_sec).min(capacity as f64);
        self.last_refill = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            return ChannelRateLimitDecision {
                allowed: true,
                retry_after_secs: 0,
            };
        }
        let missing = 1.0 - self.tokens;
        let retry_after_secs = (missing / refill_per_sec).ceil().max(1.0) as u64;
        ChannelRateLimitDecision {
            allowed: false,
            retry_after_secs,
        }
    }
}

#[derive(Debug, Default)]
pub struct ChannelRateLimiter {
    buckets: RwLock<HashMap<String, TokenBucket>>,
}

impl ChannelRateLimiter {
    pub async fn check(
        &self,
        key: &ChannelRateLimitKey,
        kind: ChannelRateLimitKind,
        profile: ChannelSecurityProfile,
    ) -> ChannelRateLimitDecision {
        let capacity = rate_limit_capacity(kind, profile);
        let refill_per_sec = capacity as f64 / 60.0;
        let bucket_key = format!(
            "{}:{}:{}",
            key.channel.trim().to_ascii_lowercase(),
            key.user_id.trim().to_ascii_lowercase(),
            kind.as_str()
        );
        let now = Instant::now();
        let mut guard = self.buckets.write().await;
        guard
            .entry(bucket_key)
            .or_insert_with(|| TokenBucket::new(capacity, now))
            .check(capacity, refill_per_sec, now)
    }
}

impl ChannelRateLimitKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Decision => "decision",
        }
    }
}

pub fn channel_rate_limit_key_from_session_metadata(
    metadata: Option<&Value>,
) -> Option<ChannelRateLimitKey> {
    let metadata = metadata?;
    let channel = metadata
        .get("channel")
        .or_else(|| metadata.get("source_platform"))
        .and_then(Value::as_str)?
        .trim();
    let user_id = metadata
        .get("user_id")
        .or_else(|| metadata.get("surface_user_id"))
        .or_else(|| metadata.get("sender_id"))
        .and_then(Value::as_str)?
        .trim();
    if channel.is_empty() || user_id.is_empty() {
        return None;
    }
    Some(ChannelRateLimitKey {
        channel: channel.to_ascii_lowercase(),
        user_id: user_id.to_string(),
    })
}

pub fn channel_rate_limit_key_from_principal(
    principal: &RequestPrincipal,
) -> Option<ChannelRateLimitKey> {
    let actor_id = principal.actor_id.as_deref()?;
    let mut parts = actor_id.splitn(4, ':');
    if parts.next()? != "channel" {
        return None;
    }
    let channel = parts.next()?.trim();
    let user_id = parts.next()?.trim();
    if channel.is_empty() || user_id.is_empty() {
        return None;
    }
    Some(ChannelRateLimitKey {
        channel: channel.to_ascii_lowercase(),
        user_id: user_id.to_string(),
    })
}

pub fn rate_limit_capacity(kind: ChannelRateLimitKind, profile: ChannelSecurityProfile) -> u32 {
    let base_env_name = match kind {
        ChannelRateLimitKind::Prompt => "TANDEM_CHANNEL_PROMPT_RATE_LIMIT_PER_MINUTE",
        ChannelRateLimitKind::Decision => "TANDEM_CHANNEL_DECISION_RATE_LIMIT_PER_MINUTE",
    };
    let profile_env_name = format!(
        "{}_{}",
        base_env_name,
        match profile {
            ChannelSecurityProfile::Operator => "OPERATOR",
            ChannelSecurityProfile::TrustedTeam => "TRUSTED_TEAM",
            ChannelSecurityProfile::PublicDemo => "PUBLIC_DEMO",
        }
    );
    read_positive_u32_env(&profile_env_name)
        .or_else(|| read_positive_u32_env(base_env_name))
        .unwrap_or(match kind {
            ChannelRateLimitKind::Prompt => DEFAULT_PROMPT_LIMIT_PER_MINUTE,
            ChannelRateLimitKind::Decision => DEFAULT_DECISION_LIMIT_PER_MINUTE,
        })
}

pub fn retry_after_duration(decision: ChannelRateLimitDecision) -> Duration {
    Duration::from_secs(decision.retry_after_secs.max(1))
}

fn read_positive_u32_env(name: &str) -> Option<u32> {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<u32>().ok())
        .filter(|value| *value > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_channel_rate_key_from_session_metadata() {
        let key = channel_rate_limit_key_from_session_metadata(Some(&json!({
            "channel": "Telegram",
            "user_id": "42"
        })))
        .unwrap();
        assert_eq!(key.channel, "telegram");
        assert_eq!(key.user_id, "42");
    }

    #[test]
    fn extracts_channel_rate_key_from_principal() {
        let principal = RequestPrincipal {
            actor_id: Some("channel:slack:U123".to_string()),
            source: "channel:slack".to_string(),
        };
        let key = channel_rate_limit_key_from_principal(&principal).unwrap();
        assert_eq!(key.channel, "slack");
        assert_eq!(key.user_id, "U123");
    }

    #[tokio::test]
    async fn eleventh_prompt_is_limited_by_default() {
        let limiter = ChannelRateLimiter::default();
        let key = ChannelRateLimitKey {
            channel: "telegram".to_string(),
            user_id: "42".to_string(),
        };
        for _ in 0..DEFAULT_PROMPT_LIMIT_PER_MINUTE {
            let decision = limiter
                .check(
                    &key,
                    ChannelRateLimitKind::Prompt,
                    ChannelSecurityProfile::PublicDemo,
                )
                .await;
            assert!(decision.allowed);
        }
        let decision = limiter
            .check(
                &key,
                ChannelRateLimitKind::Prompt,
                ChannelSecurityProfile::PublicDemo,
            )
            .await;
        assert!(!decision.allowed);
        assert!(decision.retry_after_secs > 0);
    }
}
