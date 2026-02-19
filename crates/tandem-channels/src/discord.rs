//! Discord channel adapter for Tandem.
//!
//! Connects to the Discord Gateway WebSocket, sends an Identify payload,
//! maintains a heartbeat loop, and dispatches `MESSAGE_CREATE` events.
//! Messages are split into 2000-character chunks (Unicode-aware) to comply
//! with Discord's limit.

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::{is_user_allowed, DiscordConfig};
use crate::traits::{Channel, ChannelMessage, SendMessage};

/// Discord's maximum message length for regular messages.
const DISCORD_MAX_MESSAGE_LENGTH: usize = 2000;
const DISCORD_API: &str = "https://discord.com/api/v10";

// ---------------------------------------------------------------------------
// Message splitting
// ---------------------------------------------------------------------------

/// Split a message into chunks that respect Discord's 2000-character limit.
/// Tries to split at newline > space > hard boundary.
pub fn split_message(message: &str) -> Vec<String> {
    if message.chars().count() <= DISCORD_MAX_MESSAGE_LENGTH {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;

    while !remaining.is_empty() {
        let hard_split = remaining
            .char_indices()
            .nth(DISCORD_MAX_MESSAGE_LENGTH)
            .map_or(remaining.len(), |(idx, _)| idx);

        let chunk_end = if hard_split == remaining.len() {
            hard_split
        } else {
            let search_area = &remaining[..hard_split];
            if let Some(pos) = search_area.rfind('\n') {
                if search_area[..pos].chars().count() >= DISCORD_MAX_MESSAGE_LENGTH / 2 {
                    pos + 1
                } else {
                    search_area.rfind(' ').map_or(hard_split, |s| s + 1)
                }
            } else if let Some(pos) = search_area.rfind(' ') {
                pos + 1
            } else {
                hard_split
            }
        };

        chunks.push(remaining[..chunk_end].to_string());
        remaining = &remaining[chunk_end..];
    }

    chunks
}

// ---------------------------------------------------------------------------
// Bot-mention normalization
// ---------------------------------------------------------------------------

fn mention_tags(bot_user_id: &str) -> [String; 2] {
    [format!("<@{bot_user_id}>"), format!("<@!{bot_user_id}>")]
}

fn normalize_incoming_content(
    content: &str,
    mention_only: bool,
    bot_user_id: &str,
) -> Option<String> {
    if content.is_empty() {
        return None;
    }
    let tags = mention_tags(bot_user_id);
    let is_mentioned = tags.iter().any(|t| content.contains(t.as_str()));

    if mention_only && !is_mentioned {
        return None;
    }

    let mut normalized = content.to_string();
    if mention_only {
        for tag in &tags {
            normalized = normalized.replace(tag.as_str(), " ");
        }
    }

    let normalized = normalized.trim().to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

// ---------------------------------------------------------------------------
// Token → bot user ID (minimal base64 decode — no extra dep)
// ---------------------------------------------------------------------------

const BASE64_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[allow(clippy::cast_possible_truncation)]
fn base64_decode(input: &str) -> Option<String> {
    let padded = match input.len() % 4 {
        2 => format!("{input}=="),
        3 => format!("{input}="),
        _ => input.to_string(),
    };

    let mut bytes = Vec::new();
    let chars: Vec<u8> = padded.bytes().collect();

    for chunk in chars.chunks(4) {
        if chunk.len() < 4 {
            break;
        }
        let mut v = [0usize; 4];
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'=' {
                v[i] = 0;
            } else {
                v[i] = BASE64_ALPHABET.iter().position(|&a| a == b)?;
            }
        }
        bytes.push(((v[0] << 2) | (v[1] >> 4)) as u8);
        if chunk[2] != b'=' {
            bytes.push((((v[1] & 0xF) << 4) | (v[2] >> 2)) as u8);
        }
        if chunk[3] != b'=' {
            bytes.push((((v[2] & 0x3) << 6) | v[3]) as u8);
        }
    }

    String::from_utf8(bytes).ok()
}

fn bot_user_id_from_token(token: &str) -> Option<String> {
    let part = token.split('.').next()?;
    base64_decode(part)
}

// ---------------------------------------------------------------------------
// DiscordChannel
// ---------------------------------------------------------------------------

pub struct DiscordChannel {
    bot_token: String,
    guild_id: Option<String>,
    allowed_users: Vec<String>,
    mention_only: bool,
    /// Typing indicator handle — single per-channel (Discord typing is per channel).
    typing_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl DiscordChannel {
    pub fn new(config: DiscordConfig) -> Self {
        Self {
            bot_token: config.bot_token,
            guild_id: config.guild_id,
            allowed_users: config.allowed_users,
            mention_only: config.mention_only,
            typing_handle: Mutex::new(None),
        }
    }

    fn http_client(&self) -> Client {
        Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build reqwest client")
    }

    fn auth_header(&self) -> String {
        format!("Bot {}", self.bot_token)
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let client = self.http_client();
        let chunks = split_message(&message.content);

        for (i, chunk) in chunks.iter().enumerate() {
            let url = format!("{DISCORD_API}/channels/{}/messages", message.recipient);
            let resp = client
                .post(&url)
                .header("Authorization", self.auth_header())
                .json(&json!({ "content": chunk }))
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let err = resp.text().await.unwrap_or_default();
                anyhow::bail!("Discord send failed ({status}): {err}");
            }

            // Small inter-chunk delay to avoid rate limiting
            if i < chunks.len() - 1 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        let bot_user_id = bot_user_id_from_token(&self.bot_token).unwrap_or_default();

        // Fetch gateway URL
        let gw_resp: serde_json::Value = self
            .http_client()
            .get(format!("{DISCORD_API}/gateway/bot"))
            .header("Authorization", self.auth_header())
            .send()
            .await?
            .json()
            .await?;

        let gw_url = gw_resp
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("wss://gateway.discord.gg");

        let ws_url = format!("{gw_url}/?v=10&encoding=json");
        info!("Discord: connecting to gateway {ws_url}");

        let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url).await?;
        let (mut write, mut read) = ws_stream.split();

        // Read Hello (op 10)
        let hello = read
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("Discord: no Hello received"))??;
        let hello_data: serde_json::Value = serde_json::from_str(&hello.to_string())?;
        let heartbeat_interval = hello_data
            .get("d")
            .and_then(|d| d.get("heartbeat_interval"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(41_250);

        // Send Identify (op 2)
        // Intents: 37377 = GUILDS | GUILD_MESSAGES | MESSAGE_CONTENT | DIRECT_MESSAGES
        let identify = json!({
            "op": 2,
            "d": {
                "token": self.bot_token,
                "intents": 37377,
                "properties": {
                    "os": "linux",
                    "browser": "tandem",
                    "device": "tandem"
                }
            }
        });
        write
            .send(Message::Text(identify.to_string().into()))
            .await?;
        info!("Discord: identified, heartbeat every {heartbeat_interval}ms");

        // Heartbeat timer — sends ticks into the select! loop
        let (hb_tx, mut hb_rx) = tokio::sync::mpsc::channel::<()>(1);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(heartbeat_interval));
            loop {
                interval.tick().await;
                if hb_tx.send(()).await.is_err() {
                    break;
                }
            }
        });

        let guild_filter = self.guild_id.clone();
        let mut sequence: i64 = -1;

        loop {
            tokio::select! {
                _ = hb_rx.recv() => {
                    let d = if sequence >= 0 { json!(sequence) } else { json!(null) };
                    let hb = json!({"op": 1, "d": d});
                    if write.send(Message::Text(hb.to_string().into())).await.is_err() {
                        break;
                    }
                }
                msg = read.next() => {
                    let text = match msg {
                        Some(Ok(Message::Text(t))) => t,
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => continue,
                    };

                    let event: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(e) => e,
                        Err(_) => continue,
                    };

                    if let Some(s) = event.get("s").and_then(serde_json::Value::as_i64) {
                        sequence = s;
                    }

                    let op = event.get("op").and_then(serde_json::Value::as_u64).unwrap_or(0);
                    match op {
                        1 => {
                            // Server requests immediate heartbeat
                            let d = if sequence >= 0 { json!(sequence) } else { json!(null) };
                            if write.send(Message::Text(json!({"op":1,"d":d}).to_string().into())).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        7 => {
                            warn!("Discord: Reconnect (op 7), restarting");
                            break;
                        }
                        9 => {
                            warn!("Discord: Invalid Session (op 9), restarting");
                            break;
                        }
                        _ => {}
                    }

                    let t = event.get("t").and_then(|t| t.as_str()).unwrap_or("");
                    if t != "MESSAGE_CREATE" {
                        continue;
                    }

                    let Some(d) = event.get("d") else { continue };

                    // Filter out own messages
                    let author_id = d["author"]["id"].as_str().unwrap_or("");
                    if author_id == bot_user_id {
                        continue;
                    }

                    // Filter out other bots
                    if d["author"]["bot"].as_bool().unwrap_or(false) {
                        continue;
                    }

                    // Allowlist
                    if !is_user_allowed(author_id, &self.allowed_users) {
                        warn!("Discord: ignoring message from unauthorized user {author_id}");
                        continue;
                    }

                    // Guild filter — let DMs through (no guild_id)
                    if let Some(ref gid) = guild_filter {
                        if let Some(msg_guild) = d.get("guild_id").and_then(serde_json::Value::as_str) {
                            if msg_guild != gid {
                                continue;
                            }
                        }
                    }

                    let content = d["content"].as_str().unwrap_or("");
                    let Some(clean_content) =
                        normalize_incoming_content(content, self.mention_only, &bot_user_id)
                    else {
                        continue;
                    };

                    let message_id = d["id"].as_str().unwrap_or("");
                    let channel_id = d["channel_id"].as_str().unwrap_or("").to_string();

                    let channel_msg = ChannelMessage {
                        id: if message_id.is_empty() {
                            Uuid::new_v4().to_string()
                        } else {
                            format!("discord_{message_id}")
                        },
                        sender: author_id.to_string(),
                        reply_target: if channel_id.is_empty() {
                            author_id.to_string()
                        } else {
                            channel_id
                        },
                        content: clean_content,
                        channel: "discord".to_string(),
                        timestamp: chrono::Utc::now(),
                        attachment: None,
                    };

                    if tx.send(channel_msg).await.is_err() {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> bool {
        self.http_client()
            .get(format!("{DISCORD_API}/users/@me"))
            .header("Authorization", self.auth_header())
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
        // Abort any previous handle first
        self.stop_typing(recipient).await?;

        let client = self.http_client();
        let token = self.bot_token.clone();
        let channel_id = recipient.to_string();

        let handle = tokio::spawn(async move {
            let url = format!("{DISCORD_API}/channels/{channel_id}/typing");
            loop {
                let _ = client
                    .post(&url)
                    .header("Authorization", format!("Bot {token}"))
                    .send()
                    .await;
                tokio::time::sleep(Duration::from_secs(8)).await;
            }
        });

        *self.typing_handle.lock() = Some(handle);
        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        if let Some(handle) = self.typing_handle.lock().take() {
            handle.abort();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_channel() -> DiscordChannel {
        DiscordChannel {
            bot_token: "fake".into(),
            guild_id: None,
            allowed_users: vec![],
            mention_only: false,
            typing_handle: Mutex::new(None),
        }
    }

    // ── Allowlist ────────────────────────────────────────────────────

    #[test]
    fn empty_allowlist_denies_everyone() {
        let ch = make_channel();
        assert!(!is_user_allowed("12345", &ch.allowed_users));
    }

    #[test]
    fn wildcard_allows_everyone() {
        let ch = DiscordChannel {
            allowed_users: vec!["*".into()],
            ..make_channel()
        };
        assert!(is_user_allowed("12345", &ch.allowed_users));
    }

    #[test]
    fn specific_allowlist_filters() {
        let ch = DiscordChannel {
            allowed_users: vec!["111".into(), "222".into()],
            ..make_channel()
        };
        assert!(is_user_allowed("111", &ch.allowed_users));
        assert!(!is_user_allowed("333", &ch.allowed_users));
    }

    // ── Base64 / token parsing ────────────────────────────────────────

    #[test]
    fn base64_decode_bot_id() {
        assert_eq!(base64_decode("MTIzNDU2"), Some("123456".to_string()));
    }

    #[test]
    fn bot_user_id_extraction() {
        let token = "MTIzNDU2.fake.hmac";
        assert_eq!(bot_user_id_from_token(token), Some("123456".to_string()));
    }

    #[test]
    fn base64_decode_invalid_chars() {
        assert!(base64_decode("!!!!").is_none());
    }

    // ── Mention normalization ────────────────────────────────────────

    #[test]
    fn normalize_strips_bot_mention() {
        let cleaned = normalize_incoming_content("  <@!12345> run status  ", true, "12345");
        assert_eq!(cleaned.as_deref(), Some("run status"));
    }

    #[test]
    fn normalize_requires_mention_when_enabled() {
        let cleaned = normalize_incoming_content("hello there", true, "12345");
        assert!(cleaned.is_none());
    }

    #[test]
    fn normalize_rejects_empty_after_strip() {
        let cleaned = normalize_incoming_content("<@12345>", true, "12345");
        assert!(cleaned.is_none());
    }

    #[test]
    fn normalize_no_mention_filter_passes_all() {
        let cleaned = normalize_incoming_content("hello", false, "12345");
        assert_eq!(cleaned.as_deref(), Some("hello"));
    }

    // ── Message splitting ─────────────────────────────────────────────

    #[test]
    fn split_short_message() {
        assert_eq!(split_message("Hello!"), vec!["Hello!".to_string()]);
    }

    #[test]
    fn split_exactly_at_limit() {
        let msg = "a".repeat(DISCORD_MAX_MESSAGE_LENGTH);
        let chunks = split_message(&msg);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn split_just_over_limit() {
        let msg = "a".repeat(DISCORD_MAX_MESSAGE_LENGTH + 1);
        let chunks = split_message(&msg);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), DISCORD_MAX_MESSAGE_LENGTH);
    }

    #[test]
    fn split_very_long_message_preserves_content() {
        let orig = "word ".repeat(2000);
        let chunks = split_message(&orig);
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(chunk.chars().count() <= DISCORD_MAX_MESSAGE_LENGTH);
        }
        assert_eq!(chunks.concat(), orig);
    }

    #[test]
    fn split_prefers_newline_break() {
        let msg = format!("{}\n{}", "a".repeat(1500), "b".repeat(500));
        let chunks = split_message(&msg);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].ends_with('\n'));
    }

    #[test]
    fn split_unicode_emoji() {
        let msg = "🦀 Rust! ".repeat(500);
        let chunks = split_message(&msg);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= DISCORD_MAX_MESSAGE_LENGTH);
        }
        assert_eq!(chunks.concat(), msg);
    }

    // ── Typing ───────────────────────────────────────────────────────

    #[test]
    fn typing_handle_starts_as_none() {
        let ch = make_channel();
        assert!(ch.typing_handle.lock().is_none());
    }

    #[tokio::test]
    async fn start_typing_sets_handle() {
        let ch = make_channel();
        let _ = ch.start_typing("123456").await;
        assert!(ch.typing_handle.lock().is_some());
    }

    #[tokio::test]
    async fn stop_typing_clears_handle() {
        let ch = make_channel();
        let _ = ch.start_typing("123456").await;
        let _ = ch.stop_typing("123456").await;
        assert!(ch.typing_handle.lock().is_none());
    }

    #[tokio::test]
    async fn stop_typing_is_idempotent() {
        let ch = make_channel();
        assert!(ch.stop_typing("123456").await.is_ok());
        assert!(ch.stop_typing("123456").await.is_ok());
    }
}
