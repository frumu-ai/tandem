//! Core trait definitions for Tandem channel adapters.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    SlashCommand,
    DirectMessage,
    Mention,
    ReplyToBot,
    Ambient,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageTriggerContext {
    pub source: TriggerSource,
    pub is_direct_message: bool,
    pub was_explicitly_mentioned: bool,
    pub is_reply_to_bot: bool,
}

impl Default for MessageTriggerContext {
    fn default() -> Self {
        Self {
            source: TriggerSource::Ambient,
            is_direct_message: false,
            was_explicitly_mentioned: false,
            is_reply_to_bot: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationScopeKind {
    Direct,
    Room,
    Thread,
    Topic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConversationScope {
    pub kind: ConversationScopeKind,
    pub id: String,
}

impl Default for ConversationScope {
    fn default() -> Self {
        Self {
            kind: ConversationScopeKind::Room,
            id: "room:unknown".to_string(),
        }
    }
}

pub fn should_accept_message(
    mention_only: bool,
    trigger: &MessageTriggerContext,
    has_text: bool,
    has_attachment: bool,
) -> bool {
    if !has_text && !has_attachment {
        return false;
    }
    if !mention_only {
        return true;
    }
    trigger.is_direct_message
        || trigger.was_explicitly_mentioned
        || trigger.is_reply_to_bot
        || matches!(trigger.source, TriggerSource::SlashCommand)
}

/// A message received from an external channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    /// Unique ID for this message (platform-provided).
    pub id: String,
    /// The sender's identifier on the platform (username, user_id, etc.).
    pub sender: String,
    /// Where to send the reply (chat_id, channel_id, etc. — platform-specific).
    pub reply_target: String,
    /// Plain-text message content, with any bot-mention prefix stripped.
    pub content: String,
    /// Name of the originating channel adapter (e.g. `"telegram"`, `"discord"`).
    pub channel: String,
    /// When the message was sent on the platform.
    pub timestamp: DateTime<Utc>,
    /// Optional raw attachment description (file name, URL, etc.)
    pub attachment: Option<String>,
    /// Optional attachment URL when the platform provides one.
    pub attachment_url: Option<String>,
    /// Optional local filesystem path where the adapter stored the attachment.
    pub attachment_path: Option<String>,
    /// Optional MIME type for the attachment.
    pub attachment_mime: Option<String>,
    /// Optional attachment filename.
    pub attachment_filename: Option<String>,
    /// Structured information about how this message targeted the bot.
    #[serde(default)]
    pub trigger: MessageTriggerContext,
    /// Stable conversation scope used for session identity.
    #[serde(default)]
    pub scope: ConversationScope,
}

/// A message to send back to the external channel.
#[derive(Debug, Clone)]
pub struct SendMessage {
    /// Text content to deliver. Adapters must chunk this to platform limits.
    pub content: String,
    /// Destination (chat_id, channel_id, user_id, etc. — platform-specific).
    pub recipient: String,
    /// Optional image URLs to send alongside text.
    pub image_urls: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ThreadReply {
    pub content: String,
    pub recipient: String,
    pub thread_id: String,
}

/// A rich, interactive card sent back to the channel — typically an approval
/// request, a draft confirmation, or a status update with action buttons.
///
/// Each adapter renders this to its native interactive primitive (Slack Block
/// Kit, Discord embed + action row, Telegram inline keyboard). The same
/// `InteractiveCard` shape produces a coherent UX across all three channels.
///
/// Buttons carry an `action_id` that round-trips through the platform's
/// callback (Slack interactions, Discord interactions, Telegram callback_query)
/// and identifies which decision the user took.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveCard {
    /// Destination (channel_id, chat_id, user_id, etc. — platform-specific).
    pub recipient: String,
    /// Short title shown in the card header (e.g. workflow name + step).
    pub title: String,
    /// Markdown body explaining what is about to happen. Adapters convert to
    /// the closest native rich-text format (Slack mrkdwn, Telegram MarkdownV2,
    /// Discord embed description).
    pub body_markdown: String,
    /// Key-value rows shown below the body (e.g. `run_id`, requested-by,
    /// expires-at). Order is preserved.
    #[serde(default)]
    pub fields: Vec<InteractiveCardField>,
    /// Action buttons. Render order from left to right. Adapters that cap the
    /// number of buttons per row (Discord: 5) chunk into multiple rows.
    #[serde(default)]
    pub buttons: Vec<InteractiveCardButton>,
    /// If `Some`, the button with `requires_reason: true` triggers a follow-up
    /// reason prompt with this configuration. Slack/Discord render as a modal;
    /// Telegram falls back to `force_reply`.
    #[serde(default)]
    pub reason_prompt: Option<InteractiveCardReasonPrompt>,
    /// Optional thread/topic key to anchor the card and subsequent updates
    /// inside a per-run thread. Adapters that support threading (Slack, Discord)
    /// use this to keep channels uncluttered; Telegram ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_key: Option<String>,
    /// Opaque correlation context the interaction handler will receive back
    /// when a button is clicked. Includes `run_id`, `node_id`, and any
    /// surface-payload bits the aggregator stamped earlier.
    #[serde(default)]
    pub correlation: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveCardField {
    pub label: String,
    pub value: String,
}

/// Visual / semantic intent for a button. Each adapter maps to its native
/// equivalent (Slack `style: primary|danger`, Discord button styles).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InteractiveCardButtonStyle {
    /// Default neutral button.
    Default,
    /// Affirmative / safe action (Slack `primary`, Discord `Success` green).
    Primary,
    /// Destructive / irreversible action (Slack `danger`, Discord `Danger` red).
    Destructive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveCardButton {
    /// Stable action identifier round-tripped through the platform callback.
    /// Convention: `approve`, `rework`, `cancel` for approval cards.
    pub action_id: String,
    /// Label shown on the button.
    pub label: String,
    /// Visual style.
    #[serde(default = "default_button_style")]
    pub style: InteractiveCardButtonStyle,
    /// If true, click triggers the `reason_prompt` modal/force-reply before
    /// the decision is committed. Used by Rework so the user explains why.
    #[serde(default)]
    pub requires_reason: bool,
    /// Optional confirm dialog (title + body + ok/cancel labels). Adapters
    /// render where supported (Slack `confirm` block, Discord followup modal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<InteractiveCardConfirm>,
}

fn default_button_style() -> InteractiveCardButtonStyle {
    InteractiveCardButtonStyle::Default
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveCardConfirm {
    pub title: String,
    pub body: String,
    pub confirm_label: String,
    pub deny_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveCardReasonPrompt {
    pub modal_title: String,
    pub field_label: String,
    pub field_placeholder: Option<String>,
    pub submit_label: String,
}

/// Returned from `Channel::send_card` to give the dispatcher / engine a handle
/// for later in-place edits ("Approved by @alice at 14:32") and threaded
/// status updates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveCardSent {
    /// Adapter name (`slack`, `discord`, `telegram`).
    pub channel: String,
    /// Native message ID returned by the platform (Slack `ts`, Telegram
    /// `message_id`, Discord message snowflake).
    pub message_id: String,
    /// Recipient/destination this was delivered to.
    pub recipient: String,
    /// Thread anchor when the adapter created or used a thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

/// Error returned by `Channel::send_card` when the adapter does not support
/// rich interactive rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InteractiveCardError {
    /// Adapter has not implemented `send_card` yet — caller should fall back
    /// to a text-only `send`.
    NotImplemented,
    /// Adapter accepted the card but the platform rejected it (rate limit,
    /// invalid block, etc.). Carries a short, user-safe reason.
    PlatformError(String),
    /// The card's content violates a precondition (oversize body, more
    /// buttons than the adapter can render, etc.).
    InvalidCard(String),
}

impl core::fmt::Display for InteractiveCardError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotImplemented => write!(f, "channel does not implement send_card"),
            Self::PlatformError(reason) => write!(f, "channel platform error: {reason}"),
            Self::InvalidCard(reason) => write!(f, "invalid interactive card: {reason}"),
        }
    }
}

impl std::error::Error for InteractiveCardError {}

/// All external channel adapters implement this trait.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Short lowercase adapter name, e.g. `"telegram"`, `"discord"`, `"slack"`.
    fn name(&self) -> &str;

    /// Send a message to the given recipient.
    async fn send(&self, message: &SendMessage) -> anyhow::Result<()>;

    async fn send_thread_reply(&self, reply: &ThreadReply) -> anyhow::Result<()> {
        self.send(&SendMessage {
            content: reply.content.clone(),
            recipient: reply.recipient.clone(),
            image_urls: Vec::new(),
        })
        .await
    }

    /// Listen for incoming messages and forward them through `tx`.
    ///
    /// This method should run until the sender is dropped or an unrecoverable
    /// error occurs. The supervisor in `dispatcher.rs` handles restarts.
    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()>;

    /// Returns `true` if the platform connection is currently healthy.
    /// Used by the supervisor to decide whether to log a warning on restart.
    async fn health_check(&self) -> bool {
        true
    }

    /// Begin showing a typing indicator to the recipient. A background task
    /// must be started here and tracked so `stop_typing` can abort it.
    async fn start_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// Cancel the active typing indicator for the recipient.
    async fn stop_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// `true` if the platform supports in-place message editing for streaming
    /// partial responses. Used to enable draft-update mode in the dispatcher.
    fn supports_draft_updates(&self) -> bool {
        false
    }

    /// Send a rich interactive card (approval request, status update with
    /// buttons, etc.). Default impl returns `NotImplemented` so the type
    /// system tells callers which adapters have implemented rich rendering.
    /// Callers should fall back to a text-only `send` when this returns
    /// `InteractiveCardError::NotImplemented`.
    async fn send_card(
        &self,
        _card: &InteractiveCard,
    ) -> Result<InteractiveCardSent, InteractiveCardError> {
        Err(InteractiveCardError::NotImplemented)
    }

    /// `true` when this adapter implements `send_card` end-to-end. The default
    /// returns `false`; adapters override when their `send_card` is wired.
    /// Used by the notification fan-out task to decide between rich card and
    /// text fallback without making a wasted API call.
    fn supports_interactive_cards(&self) -> bool {
        false
    }
}
