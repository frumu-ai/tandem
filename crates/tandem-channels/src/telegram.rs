//! Telegram channel adapter for Tandem.
//!
//! Uses the Bot API long-polling (`getUpdates` with `timeout=25`) to receive
//! messages and `sendMessage` to deliver responses. Messages are split into
//! 4096-character chunks to comply with Telegram's limit.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use reqwest::Client;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};

use crate::config::{is_user_allowed, TelegramConfig};
use crate::traits::{Channel, ChannelMessage, SendMessage};

const MAX_MESSAGE_LEN: usize = 4096;
const TELEGRAM_API: &str = "https://api.telegram.org/bot";

/// Split a long message into ≤4096-character chunks.
pub fn split_message(text: &str) -> Vec<String> {
    if text.len() <= MAX_MESSAGE_LEN {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        // Try to split on a newline boundary within the last 200 chars of the chunk.
        let end = (start + MAX_MESSAGE_LEN).min(text.len());
        let chunk = &text[start..end];
        let split_at = if end < text.len() {
            chunk.rfind('\n').map(|p| p + 1).unwrap_or(chunk.len())
        } else {
            chunk.len()
        };
        chunks.push(text[start..start + split_at].to_string());
        start += split_at;
    }
    chunks
}

pub struct TelegramChannel {
    bot_token: String,
    allowed_users: Vec<String>,
    mention_only: bool,
    client: Client,
    typing_handles: Arc<Mutex<std::collections::HashMap<String, JoinHandle<()>>>>,
}

impl TelegramChannel {
    pub fn new(config: TelegramConfig) -> Self {
        Self {
            bot_token: config.bot_token,
            allowed_users: config.allowed_users,
            mention_only: config.mention_only,
            client: Client::builder()
                .timeout(Duration::from_secs(35))
                .build()
                .expect("failed to create reqwest client"),
            typing_handles: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}{}/{}", TELEGRAM_API, self.bot_token, method)
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        for chunk in split_message(&message.content) {
            let body = serde_json::json!({
                "chat_id": message.recipient,
                "text": chunk,
                "parse_mode": "Markdown",
            });
            let resp = self
                .client
                .post(self.api_url("sendMessage"))
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                let text = resp.text().await.unwrap_or_default();
                error!("telegram sendMessage failed: {text}");
            }
        }
        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let mut offset: i64 = 0;
        loop {
            let resp = self
                .client
                .get(self.api_url("getUpdates"))
                .query(&[
                    ("timeout", "25"),
                    ("offset", &offset.to_string()),
                    ("allowed_updates", r#"["message"]"#),
                ])
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    warn!("telegram poll error: {e}");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            let json: Value = match resp.json().await {
                Ok(v) => v,
                Err(e) => {
                    warn!("telegram json parse error: {e}");
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            };

            let updates = match json.get("result").and_then(|r| r.as_array()) {
                Some(u) => u.clone(),
                None => {
                    debug!("telegram: no result array");
                    continue;
                }
            };

            for update in &updates {
                let update_id = update["update_id"].as_i64().unwrap_or(0);
                offset = offset.max(update_id + 1);

                let msg = match update.get("message") {
                    Some(m) => m,
                    None => continue,
                };

                let text = match msg.get("text").and_then(|t| t.as_str()) {
                    Some(t) => t,
                    None => continue,
                };

                let chat_id = msg["chat"]["id"].as_i64().unwrap_or(0).to_string();

                // Sender = username or first_name fallback
                let sender = msg["from"]["username"]
                    .as_str()
                    .map(|u| format!("@{u}"))
                    .or_else(|| msg["from"]["first_name"].as_str().map(|n| n.to_string()))
                    .unwrap_or_else(|| msg["from"]["id"].to_string());

                if !is_user_allowed(&sender, &self.allowed_users) {
                    debug!("telegram: ignoring message from {sender} (not in allowed_users)");
                    continue;
                }

                // Strip bot-mention prefix if present
                let content = if self.mention_only {
                    // Bot mention looks like "@botname text"
                    text.splitn(2, ' ')
                        .nth(1)
                        .unwrap_or(text)
                        .trim()
                        .to_string()
                } else {
                    text.to_string()
                };

                if content.is_empty() {
                    continue;
                }

                let channel_msg = ChannelMessage {
                    id: update_id.to_string(),
                    sender: sender.clone(),
                    reply_target: chat_id,
                    content,
                    channel: "telegram".to_string(),
                    timestamp: chrono::Utc::now(),
                    attachment: None,
                };

                if tx.send(channel_msg).await.is_err() {
                    return Ok(()); // receiver dropped — shutdown
                }
            }
        }
    }

    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
        let url = self.api_url("sendChatAction");
        let body = serde_json::json!({ "chat_id": recipient, "action": "typing" });
        let client = self.client.clone();
        let recipient = recipient.to_string();
        let handle = tokio::spawn(async move {
            loop {
                let _ = client.post(&url).json(&body).send().await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        self.typing_handles.lock().insert(recipient, handle);
        Ok(())
    }

    async fn stop_typing(&self, recipient: &str) -> anyhow::Result<()> {
        if let Some(handle) = self.typing_handles.lock().remove(recipient) {
            handle.abort();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_short_message() {
        let msg = "Hello, world!";
        assert_eq!(split_message(msg), vec![msg.to_string()]);
    }

    #[test]
    fn test_split_long_message() {
        let msg = "a".repeat(5000);
        let chunks = split_message(&msg);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.len() <= MAX_MESSAGE_LEN);
        }
        assert_eq!(chunks.join(""), msg);
    }
}
