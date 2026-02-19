//! Slack channel adapter for Tandem.
//!
//! Polls `conversations.history` every 3 seconds and tracks `last_ts` for
//! deduplication. Sends replies via `chat.postMessage`. Fetches the bot's own
//! user ID via `auth.test` to filter self-messages.

use async_trait::async_trait;
use reqwest::Client;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::config::{is_user_allowed, SlackConfig};
use crate::traits::{Channel, ChannelMessage, SendMessage};

const SLACK_API: &str = "https://slack.com/api";
const POLL_INTERVAL_SECS: u64 = 3;

pub struct SlackChannel {
    bot_token: String,
    channel_id: String,
    allowed_users: Vec<String>,
}

impl SlackChannel {
    pub fn new(config: SlackConfig) -> Self {
        Self {
            bot_token: config.bot_token,
            channel_id: config.channel_id,
            allowed_users: config.allowed_users,
        }
    }

    fn http_client(&self) -> Client {
        Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build reqwest client")
    }

    /// Fetch the bot's own Slack user ID so we can skip our own messages.
    async fn get_bot_user_id(&self) -> Option<String> {
        let resp: serde_json::Value = self
            .http_client()
            .get(format!("{SLACK_API}/auth.test"))
            .bearer_auth(&self.bot_token)
            .send()
            .await
            .ok()?
            .json()
            .await
            .ok()?;

        resp.get("user_id")
            .and_then(|u| u.as_str())
            .map(String::from)
    }
}

#[async_trait]
impl Channel for SlackChannel {
    fn name(&self) -> &str {
        "slack"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "channel": message.recipient,
            "text": message.content,
        });

        let resp = self
            .http_client()
            .post(format!("{SLACK_API}/chat.postMessage"))
            .bearer_auth(&self.bot_token)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            anyhow::bail!("Slack chat.postMessage failed ({status}): {body_text}");
        }

        // Slack returns HTTP 200 for most app-level errors; check `"ok"` field.
        let parsed: serde_json::Value = serde_json::from_str(&body_text).unwrap_or_default();
        if parsed.get("ok") == Some(&serde_json::Value::Bool(false)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Slack chat.postMessage error: {err}");
        }

        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let bot_user_id = self.get_bot_user_id().await.unwrap_or_default();
        let mut last_ts = String::new();

        info!("Slack: listening on channel #{}", self.channel_id);

        loop {
            tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;

            let mut params = vec![
                ("channel", self.channel_id.clone()),
                ("limit", "10".to_string()),
            ];
            if !last_ts.is_empty() {
                params.push(("oldest", last_ts.clone()));
            }

            let resp = match self
                .http_client()
                .get(format!("{SLACK_API}/conversations.history"))
                .bearer_auth(&self.bot_token)
                .query(&params)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    warn!("Slack poll error: {e}");
                    continue;
                }
            };

            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    warn!("Slack parse error: {e}");
                    continue;
                }
            };

            let Some(messages) = data.get("messages").and_then(|m| m.as_array()) else {
                continue;
            };

            // Messages arrive newest-first; reverse to process oldest first.
            for msg in messages.iter().rev() {
                let ts = msg.get("ts").and_then(|t| t.as_str()).unwrap_or("");
                let user = msg
                    .get("user")
                    .and_then(|u| u.as_str())
                    .unwrap_or("unknown");
                let text = msg.get("text").and_then(|t| t.as_str()).unwrap_or("");

                // Skip bot's own messages
                if !bot_user_id.is_empty() && user == bot_user_id {
                    continue;
                }

                // Skip bot/app messages (no user field or subtype = bot_message)
                if msg.get("bot_id").is_some()
                    || msg
                        .get("subtype")
                        .and_then(|s| s.as_str())
                        .map(|s| s == "bot_message")
                        .unwrap_or(false)
                {
                    continue;
                }

                // Allowlist
                if !is_user_allowed(user, &self.allowed_users) {
                    warn!("Slack: ignoring message from unauthorized user {user}");
                    continue;
                }

                // Skip empty or already-seen messages
                if text.is_empty() || ts <= last_ts.as_str() {
                    continue;
                }

                last_ts = ts.to_string();

                let channel_msg = ChannelMessage {
                    id: format!("slack_{}_{ts}", self.channel_id),
                    sender: user.to_string(),
                    reply_target: self.channel_id.clone(),
                    content: text.to_string(),
                    channel: "slack".to_string(),
                    timestamp: chrono::Utc::now(),
                    attachment: None,
                };

                if tx.send(channel_msg).await.is_err() {
                    return Ok(()); // receiver dropped — shutdown
                }
            }
        }
    }

    async fn health_check(&self) -> bool {
        self.http_client()
            .get(format!("{SLACK_API}/auth.test"))
            .bearer_auth(&self.bot_token)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_channel() -> SlackChannel {
        SlackChannel {
            bot_token: "xoxb-fake".into(),
            channel_id: "C0FAKE".into(),
            allowed_users: vec![],
        }
    }

    #[test]
    fn channel_name() {
        assert_eq!(make_channel().name(), "slack");
    }

    #[test]
    fn empty_allowlist_denies_everyone() {
        let ch = make_channel();
        assert!(!is_user_allowed("U12345", &ch.allowed_users));
    }

    #[test]
    fn wildcard_allows_everyone() {
        let ch = SlackChannel {
            allowed_users: vec!["*".into()],
            ..make_channel()
        };
        assert!(is_user_allowed("U12345", &ch.allowed_users));
    }

    #[test]
    fn specific_allowlist_filters() {
        let ch = SlackChannel {
            allowed_users: vec!["U111".into(), "U222".into()],
            ..make_channel()
        };
        assert!(is_user_allowed("U111", &ch.allowed_users));
        assert!(!is_user_allowed("U333", &ch.allowed_users));
    }

    #[test]
    fn allowlist_exact_match() {
        let ch = SlackChannel {
            allowed_users: vec!["U111".into()],
            ..make_channel()
        };
        assert!(!is_user_allowed("U1111", &ch.allowed_users));
        assert!(!is_user_allowed("U11", &ch.allowed_users));
    }

    #[test]
    fn allowlist_case_sensitive() {
        let ch = SlackChannel {
            allowed_users: vec!["U111".into()],
            ..make_channel()
        };
        assert!(is_user_allowed("U111", &ch.allowed_users));
        assert!(!is_user_allowed("u111", &ch.allowed_users));
    }

    #[test]
    fn message_id_format() {
        let ts = "1234567890.123456";
        let channel_id = "C12345";
        let id = format!("slack_{channel_id}_{ts}");
        assert_eq!(id, "slack_C12345_1234567890.123456");
    }

    #[test]
    fn message_id_is_deterministic() {
        let ts = "1234567890.123456";
        let id1 = format!("slack_C12345_{ts}");
        let id2 = format!("slack_C12345_{ts}");
        assert_eq!(id1, id2);
    }

    #[test]
    fn message_id_different_ts_differ() {
        let id1 = format!("slack_C12345_1000.000001");
        let id2 = format!("slack_C12345_1000.000002");
        assert_ne!(id1, id2);
    }
}
