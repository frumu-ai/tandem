//! Slack channel adapter for Tandem.
//!
//! Polls `conversations.history` every 3 seconds and tracks `last_ts` for
//! deduplication. Sends replies via `chat.postMessage`. Fetches the bot's own
//! user ID via `auth.test` to filter self-messages.

use async_trait::async_trait;
use reqwest::Client;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::config::{is_user_allowed, SlackConfig};
use crate::slack_blocks;
use crate::traits::{
    should_accept_message, Channel, ChannelMessage, ConversationScope, ConversationScopeKind,
    InteractiveCard, InteractiveCardError, InteractiveCardSent, MessageTriggerContext, SendMessage,
    TriggerSource,
};

const SLACK_API: &str = "https://slack.com/api";
const POLL_INTERVAL_SECS: u64 = 3;

fn slack_attachment_description(message: &serde_json::Value) -> Option<String> {
    let files = message.get("files").and_then(serde_json::Value::as_array)?;
    if files.is_empty() {
        return None;
    }

    let first = &files[0];
    let name = first
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let count = files.len();
    if count == 1 {
        Some(format!("file:{name}"))
    } else {
        Some(format!("files:{count} (first: {name})"))
    }
}

fn slack_attachment_url(message: &serde_json::Value) -> Option<String> {
    message
        .get("files")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|f| {
            f.get("url_private_download")
                .and_then(serde_json::Value::as_str)
                .or_else(|| f.get("url_private").and_then(serde_json::Value::as_str))
        })
        .map(ToString::to_string)
}

fn slack_attachment_filename(message: &serde_json::Value) -> Option<String> {
    message
        .get("files")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|f| f.get("name").and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
}

fn slack_attachment_mime(message: &serde_json::Value) -> Option<String> {
    message
        .get("files")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|f| f.get("mimetype").and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
}

fn channel_uploads_root() -> PathBuf {
    let base = std::env::var("TANDEM_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if let Some(data_dir) = dirs::data_dir() {
                return data_dir.join("tandem").join("data");
            }
            dirs::home_dir()
                .map(|home| home.join(".tandem").join("data"))
                .unwrap_or_else(|| PathBuf::from(".tandem"))
        });
    base.join("channel_uploads")
}

fn sanitize_filename(name: &str) -> String {
    let out = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if out.is_empty() {
        "attachment.bin".to_string()
    } else {
        out
    }
}

pub struct SlackChannel {
    bot_token: String,
    channel_id: String,
    allowed_users: Vec<String>,
    mention_only: bool,
}

impl SlackChannel {
    pub fn new(config: SlackConfig) -> Self {
        Self {
            bot_token: config.bot_token,
            channel_id: config.channel_id,
            allowed_users: config.allowed_users,
            mention_only: config.mention_only,
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

    async fn download_slack_attachment(&self, url: &str, filename: Option<&str>) -> Option<String> {
        let max_bytes = std::env::var("TANDEM_CHANNEL_MAX_ATTACHMENT_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(20 * 1024 * 1024);

        let response = self
            .http_client()
            .get(url)
            .bearer_auth(&self.bot_token)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let bytes = response.bytes().await.ok()?;
        if bytes.len() as u64 > max_bytes {
            warn!(
                "slack attachment download exceeded max bytes ({} > {})",
                bytes.len(),
                max_bytes
            );
            return None;
        }

        let file_name = filename.unwrap_or("attachment.bin");
        let safe_name = sanitize_filename(file_name);
        let dir = channel_uploads_root()
            .join("slack")
            .join(sanitize_filename(&self.channel_id));
        tokio::fs::create_dir_all(&dir).await.ok()?;

        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let path = dir.join(format!("{ts}_{safe_name}"));
        tokio::fs::write(&path, &bytes).await.ok()?;
        Some(path.to_string_lossy().to_string())
    }

    pub async fn update_card_for_decision(
        &self,
        card: &InteractiveCard,
        message_ts: &str,
        decided_by_display: &str,
        decision_summary_markdown: &str,
    ) -> Result<(), InteractiveCardError> {
        let payload = slack_blocks::build_chat_update_payload_for_decision(
            card,
            message_ts,
            decided_by_display,
            decision_summary_markdown,
        );
        let resp = self
            .http_client()
            .post(format!("{SLACK_API}/chat.update"))
            .bearer_auth(&self.bot_token)
            .json(&payload)
            .send()
            .await
            .map_err(|err| InteractiveCardError::PlatformError(err.to_string()))?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InteractiveCardError::PlatformError(format!(
                "Slack chat.update failed ({status}): {body_text}"
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body_text).map_err(|err| {
            InteractiveCardError::PlatformError(format!("Slack response was not JSON: {err}"))
        })?;
        if parsed.get("ok") != Some(&serde_json::Value::Bool(true)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            return Err(InteractiveCardError::PlatformError(format!(
                "Slack chat.update error: {err}"
            )));
        }

        Ok(())
    }
}

fn normalize_slack_content(text: &str, bot_user_id: &str) -> (Option<String>, bool) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return (None, false);
    }
    if bot_user_id.is_empty() {
        return (Some(trimmed.to_string()), false);
    }
    let mention = format!("<@{bot_user_id}>");
    let was_explicitly_mentioned = trimmed.contains(&mention);
    let normalized = trimmed.replace(&mention, " ");
    let normalized = normalized.trim().to_string();
    let normalized = if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    };
    (normalized, was_explicitly_mentioned)
}

#[async_trait]
impl Channel for SlackChannel {
    fn name(&self) -> &str {
        "slack"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let mut outgoing = message.content.clone();
        for image_url in &message.image_urls {
            if !outgoing.contains(image_url) {
                if !outgoing.is_empty() {
                    outgoing.push('\n');
                }
                outgoing.push_str(image_url);
            }
        }

        let body = serde_json::json!({
            "channel": message.recipient,
            "text": outgoing,
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

    async fn send_card(
        &self,
        card: &InteractiveCard,
    ) -> Result<InteractiveCardSent, InteractiveCardError> {
        let text_fallback = if card.title.trim().is_empty() {
            "Tandem approval"
        } else {
            card.title.as_str()
        };
        let payload = slack_blocks::build_post_message_payload(
            card,
            text_fallback,
            card.thread_key.as_deref(),
        );

        let resp = self
            .http_client()
            .post(format!("{SLACK_API}/chat.postMessage"))
            .bearer_auth(&self.bot_token)
            .json(&payload)
            .send()
            .await
            .map_err(|err| InteractiveCardError::PlatformError(err.to_string()))?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(InteractiveCardError::PlatformError(format!(
                "Slack chat.postMessage failed ({status}): {body_text}"
            )));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body_text).map_err(|err| {
            InteractiveCardError::PlatformError(format!("Slack response was not JSON: {err}"))
        })?;
        if parsed.get("ok") != Some(&serde_json::Value::Bool(true)) {
            let err = parsed
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown");
            return Err(InteractiveCardError::PlatformError(format!(
                "Slack chat.postMessage error: {err}"
            )));
        }

        let message_id = parsed
            .get("ts")
            .and_then(|ts| ts.as_str())
            .ok_or_else(|| {
                InteractiveCardError::PlatformError(
                    "Slack chat.postMessage response missing ts".to_string(),
                )
            })?
            .to_string();
        let recipient = parsed
            .get("channel")
            .and_then(|channel| channel.as_str())
            .unwrap_or(&card.recipient)
            .to_string();
        let thread_id = parsed
            .get("message")
            .and_then(|message| message.get("thread_ts"))
            .and_then(|thread_ts| thread_ts.as_str())
            .map(ToString::to_string)
            .or_else(|| card.thread_key.clone());

        Ok(InteractiveCardSent {
            channel: self.name().to_string(),
            message_id,
            recipient,
            thread_id,
        })
    }

    fn supports_interactive_cards(&self) -> bool {
        true
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let bot_user_id = self.get_bot_user_id().await.unwrap_or_default();
        // Seed to current time so a restart does not re-process recent history
        // and reply to the same message multiple times. Slack ts format is
        // `<unix_seconds>.<microseconds>`.
        let mut last_ts = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
            Ok(d) => format!("{}.{:06}", d.as_secs(), d.subsec_micros()),
            Err(_) => String::new(),
        };

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
                let thread_ts = msg.get("thread_ts").and_then(|v| v.as_str());

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

                let attachment = slack_attachment_description(msg);
                let attachment_url = slack_attachment_url(msg);
                let attachment_filename = slack_attachment_filename(msg);
                let attachment_mime = slack_attachment_mime(msg);
                let (normalized_content, was_explicitly_mentioned) =
                    normalize_slack_content(text, &bot_user_id);
                let trigger = MessageTriggerContext {
                    source: if was_explicitly_mentioned {
                        TriggerSource::Mention
                    } else {
                        TriggerSource::Ambient
                    },
                    is_direct_message: false,
                    was_explicitly_mentioned,
                    is_reply_to_bot: false,
                };
                let attachment_path = if let Some(url) = attachment_url.as_deref() {
                    self.download_slack_attachment(url, attachment_filename.as_deref())
                        .await
                } else {
                    None
                };

                // Skip empty or already-seen messages
                if (text.is_empty() && attachment.is_none()) || ts <= last_ts.as_str() {
                    continue;
                }
                if !should_accept_message(
                    self.mention_only,
                    &trigger,
                    normalized_content.is_some(),
                    attachment.is_some(),
                ) {
                    continue;
                }

                last_ts = ts.to_string();
                let scope = if let Some(thread_ts) = thread_ts {
                    ConversationScope {
                        kind: ConversationScopeKind::Thread,
                        id: format!("thread:{}:{}", self.channel_id, thread_ts),
                    }
                } else {
                    ConversationScope {
                        kind: ConversationScopeKind::Room,
                        id: format!("channel:{}", self.channel_id),
                    }
                };

                let channel_msg = ChannelMessage {
                    id: format!("slack_{}_{ts}", self.channel_id),
                    sender: user.to_string(),
                    reply_target: self.channel_id.clone(),
                    content: normalized_content.unwrap_or_default(),
                    channel: "slack".to_string(),
                    timestamp: chrono::Utc::now(),
                    attachment,
                    attachment_url,
                    attachment_path,
                    attachment_mime,
                    attachment_filename,
                    trigger,
                    scope,
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
            mention_only: false,
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
    fn normalize_slack_content_strips_bot_mention() {
        let (normalized, mentioned) = normalize_slack_content("  <@Ubot> status please ", "Ubot");
        assert_eq!(normalized.as_deref(), Some("status please"));
        assert!(mentioned);
    }

    #[test]
    fn normalize_slack_content_keeps_plain_text() {
        let (normalized, mentioned) = normalize_slack_content("hello there", "Ubot");
        assert_eq!(normalized.as_deref(), Some("hello there"));
        assert!(!mentioned);
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

    #[test]
    fn detects_single_slack_file_attachment() {
        let msg = serde_json::json!({
            "files": [
                { "name": "notes.txt" }
            ]
        });
        assert_eq!(
            slack_attachment_description(&msg),
            Some("file:notes.txt".to_string())
        );
    }

    #[test]
    fn detects_multiple_slack_file_attachments() {
        let msg = serde_json::json!({
            "files": [
                { "name": "a.txt" },
                { "name": "b.png" }
            ]
        });
        assert_eq!(
            slack_attachment_description(&msg),
            Some("files:2 (first: a.txt)".to_string())
        );
    }
}
