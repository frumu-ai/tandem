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
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};
use uuid::Uuid;

use crate::config::DiscordConfig;
use crate::traits::{
    should_accept_message, Channel, ChannelMessage, ConversationScope, ConversationScopeKind,
    MessageTriggerContext, SendMessage, TriggerSource,
};

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

fn normalize_incoming_content(content: &str, bot_user_id: &str) -> (Option<String>, bool) {
    if content.is_empty() {
        return (None, false);
    }
    let tags = mention_tags(bot_user_id);
    let is_mentioned = tags.iter().any(|t| content.contains(t.as_str()));

    let mut normalized = content.to_string();
    if is_mentioned {
        for tag in &tags {
            normalized = normalized.replace(tag.as_str(), " ");
        }
    }

    let normalized = normalized.trim().to_string();
    let normalized = if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    };
    (normalized, is_mentioned)
}

fn normalize_discord_identity(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed == "*" {
        return "*".to_string();
    }
    let without_prefix = trimmed.trim_start_matches('@');
    let without_mention = without_prefix
        .strip_prefix("<@!")
        .or_else(|| without_prefix.strip_prefix("<@"))
        .unwrap_or(without_prefix)
        .trim_end_matches('>');
    without_mention.trim().to_ascii_lowercase()
}

fn is_discord_user_allowed(
    author_id: &str,
    author_username: Option<&str>,
    author_global_name: Option<&str>,
    author_discriminator: Option<&str>,
    allowed_users: &[String],
) -> bool {
    if allowed_users.is_empty() {
        return false;
    }
    if allowed_users
        .iter()
        .any(|entry| normalize_discord_identity(entry) == "*")
    {
        return true;
    }

    let mut candidates = vec![normalize_discord_identity(author_id)];
    if let Some(username) = author_username {
        if !username.trim().is_empty() {
            candidates.push(normalize_discord_identity(username));
            if let Some(discriminator) = author_discriminator {
                let discrim = discriminator.trim();
                if !discrim.is_empty() && discrim != "0" {
                    candidates.push(normalize_discord_identity(&format!("{username}#{discrim}")));
                }
            }
        }
    }
    if let Some(global_name) = author_global_name {
        if !global_name.trim().is_empty() {
            candidates.push(normalize_discord_identity(global_name));
        }
    }

    allowed_users.iter().any(|entry| {
        let allowed = normalize_discord_identity(entry);
        candidates.iter().any(|candidate| candidate == &allowed)
    })
}

fn discord_attachment_description(message: &serde_json::Value) -> Option<String> {
    let attachments = message
        .get("attachments")
        .and_then(serde_json::Value::as_array)?;
    if attachments.is_empty() {
        return None;
    }

    let first = &attachments[0];
    let filename = first
        .get("filename")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let count = attachments.len();
    if count == 1 {
        Some(format!("attachment:{filename}"))
    } else {
        Some(format!("attachments:{count} (first: {filename})"))
    }
}

fn discord_attachment_url(message: &serde_json::Value) -> Option<String> {
    message
        .get("attachments")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|a| a.get("url").and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
}

fn discord_attachment_filename(message: &serde_json::Value) -> Option<String> {
    message
        .get("attachments")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|a| a.get("filename").and_then(serde_json::Value::as_str))
        .map(ToString::to_string)
}

fn discord_attachment_mime(message: &serde_json::Value) -> Option<String> {
    message
        .get("attachments")
        .and_then(serde_json::Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|a| a.get("content_type").and_then(serde_json::Value::as_str))
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

    async fn download_discord_attachment(
        &self,
        url: &str,
        filename: Option<&str>,
        channel_id: &str,
    ) -> Option<String> {
        let max_bytes = std::env::var("TANDEM_CHANNEL_MAX_ATTACHMENT_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(20 * 1024 * 1024);

        let response = self
            .http_client()
            .get(url)
            .header("Authorization", self.auth_header())
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let bytes = response.bytes().await.ok()?;
        if bytes.len() as u64 > max_bytes {
            warn!(
                "discord attachment download exceeded max bytes ({} > {})",
                bytes.len(),
                max_bytes
            );
            return None;
        }

        let file_name = filename.unwrap_or("attachment.bin");
        let safe_name = sanitize_filename(file_name);
        let dir = channel_uploads_root()
            .join("discord")
            .join(sanitize_filename(channel_id));
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
}

#[async_trait]
impl Channel for DiscordChannel {
    fn name(&self) -> &str {
        "discord"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let client = self.http_client();
        let mut outgoing = message.content.clone();
        for image_url in &message.image_urls {
            if !outgoing.contains(image_url) {
                if !outgoing.is_empty() {
                    outgoing.push('\n');
                }
                outgoing.push_str(image_url);
            }
        }
        let chunks = split_message(&outgoing);

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
        let identify_text = identify.to_string();
        write.send(Message::Text(identify_text)).await?;
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
                    let heartbeat_text = json!({"op": 1, "d": d}).to_string();
                    if write.send(Message::Text(heartbeat_text)).await.is_err() {
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
                            let heartbeat = json!({"op":1,"d":d}).to_string();
                            if write.send(Message::Text(heartbeat)).await.is_err() {
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

                    // Allowlist: support IDs plus username/global-name for easier config.
                    let author_username = d["author"]["username"].as_str();
                    let author_global_name = d["author"]["global_name"].as_str();
                    let author_discriminator = d["author"]["discriminator"].as_str();
                    if !is_discord_user_allowed(
                        author_id,
                        author_username,
                        author_global_name,
                        author_discriminator,
                        &self.allowed_users,
                    ) {
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
                    let attachment = discord_attachment_description(d);
                    let attachment_url = discord_attachment_url(d);
                    let attachment_filename = discord_attachment_filename(d);
                    let attachment_mime = discord_attachment_mime(d);
                    let (normalized_content, was_explicitly_mentioned) =
                        normalize_incoming_content(content, &bot_user_id);
                    let is_direct_message = d.get("guild_id").is_none();
                    let is_reply_to_bot = d
                        .get("referenced_message")
                        .and_then(|msg| msg.get("author"))
                        .and_then(|author| author.get("id"))
                        .and_then(serde_json::Value::as_str)
                        .map(|id| id == bot_user_id)
                        .unwrap_or(false);
                    let trigger = MessageTriggerContext {
                        source: if is_direct_message {
                            TriggerSource::DirectMessage
                        } else if was_explicitly_mentioned {
                            TriggerSource::Mention
                        } else if is_reply_to_bot {
                            TriggerSource::ReplyToBot
                        } else {
                            TriggerSource::Ambient
                        },
                        is_direct_message,
                        was_explicitly_mentioned,
                        is_reply_to_bot,
                    };
                    if !should_accept_message(
                        self.mention_only,
                        &trigger,
                        normalized_content.is_some(),
                        attachment.is_some(),
                    ) {
                        continue;
                    }
                    let clean_content = normalized_content.unwrap_or_default();

                    let message_id = d["id"].as_str().unwrap_or("");
                    let channel_id = d["channel_id"].as_str().unwrap_or("").to_string();
                    let scope = if is_direct_message {
                        ConversationScope {
                            kind: ConversationScopeKind::Direct,
                            id: format!("dm:{}", author_id),
                        }
                    } else {
                        ConversationScope {
                            kind: ConversationScopeKind::Room,
                            id: format!("channel:{}", channel_id),
                        }
                    };
                    let attachment_path = if let Some(url) = attachment_url.as_deref() {
                        self.download_discord_attachment(
                            url,
                            attachment_filename.as_deref(),
                            &channel_id,
                        )
                        .await
                    } else {
                        None
                    };

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
                        attachment,
                        attachment_url,
                        attachment_path,
                        attachment_mime,
                        attachment_filename,
                        trigger,
                        scope,
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
        assert!(!is_discord_user_allowed(
            "12345",
            None,
            None,
            None,
            &ch.allowed_users
        ));
    }

    #[test]
    fn wildcard_allows_everyone() {
        let ch = DiscordChannel {
            allowed_users: vec!["*".into()],
            ..make_channel()
        };
        assert!(is_discord_user_allowed(
            "12345",
            Some("alice"),
            Some("Alice"),
            None,
            &ch.allowed_users
        ));
    }

    #[test]
    fn specific_allowlist_filters() {
        let ch = DiscordChannel {
            allowed_users: vec!["111".into(), "222".into()],
            ..make_channel()
        };
        assert!(is_discord_user_allowed(
            "111",
            Some("alice"),
            None,
            None,
            &ch.allowed_users
        ));
        assert!(!is_discord_user_allowed(
            "333",
            Some("alice"),
            None,
            None,
            &ch.allowed_users
        ));
    }

    #[test]
    fn username_allowlist_matches_case_insensitive() {
        let ch = DiscordChannel {
            allowed_users: vec!["@Alice".into()],
            ..make_channel()
        };
        assert!(is_discord_user_allowed(
            "999",
            Some("alice"),
            None,
            None,
            &ch.allowed_users
        ));
    }

    #[test]
    fn global_name_allowlist_matches() {
        let ch = DiscordChannel {
            allowed_users: vec!["Team Lead".into()],
            ..make_channel()
        };
        assert!(is_discord_user_allowed(
            "999",
            Some("alice"),
            Some("Team Lead"),
            None,
            &ch.allowed_users
        ));
    }

    #[test]
    fn mention_style_allowlist_matches_user_id() {
        let ch = DiscordChannel {
            allowed_users: vec!["<@!12345>".into()],
            ..make_channel()
        };
        assert!(is_discord_user_allowed(
            "12345",
            Some("alice"),
            None,
            None,
            &ch.allowed_users
        ));
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
        let (cleaned, mentioned) = normalize_incoming_content("  <@!12345> run status  ", "12345");
        assert_eq!(cleaned.as_deref(), Some("run status"));
        assert!(mentioned);
    }

    #[test]
    fn normalize_requires_mention_when_enabled() {
        let (cleaned, mentioned) = normalize_incoming_content("hello there", "12345");
        assert_eq!(cleaned.as_deref(), Some("hello there"));
        assert!(!mentioned);
    }

    #[test]
    fn normalize_rejects_empty_after_strip() {
        let (cleaned, mentioned) = normalize_incoming_content("<@12345>", "12345");
        assert!(cleaned.is_none());
        assert!(mentioned);
    }

    #[test]
    fn normalize_no_mention_filter_passes_all() {
        let (cleaned, mentioned) = normalize_incoming_content("hello", "12345");
        assert_eq!(cleaned.as_deref(), Some("hello"));
        assert!(!mentioned);
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

    #[test]
    fn detects_single_discord_attachment() {
        let d = json!({
            "attachments": [
                { "filename": "image.png" }
            ]
        });
        assert_eq!(
            discord_attachment_description(&d),
            Some("attachment:image.png".to_string())
        );
    }

    #[test]
    fn detects_multiple_discord_attachments() {
        let d = json!({
            "attachments": [
                { "filename": "a.png" },
                { "filename": "b.pdf" }
            ]
        });
        assert_eq!(
            discord_attachment_description(&d),
            Some("attachments:2 (first: a.png)".to_string())
        );
    }
}
