//! Telegram inline-keyboard renderer for [`InteractiveCard`].
//!
//! Pure functions: take an [`InteractiveCard`] in, produce JSON suitable for
//! Telegram Bot API methods (`sendMessage`, `editMessageText`,
//! `editMessageReplyMarkup`, `forceReply`).
//!
//! Telegram is the simplest of the three rich surfaces: there are no embeds
//! and no modals. We render the title + body as message text (MarkdownV2),
//! and the buttons as an inline keyboard with `callback_data` carrying our
//! correlation. After a decision, `editMessageReplyMarkup` removes the
//! buttons and `editMessageText` updates the message body to a "Decided by …"
//! summary.
//!
//! Rework reasons are captured via `forceReply: true` (Telegram's substitute
//! for a modal): the bot replies to the user with `force_reply` set, and the
//! user's NEXT message is treated as the reason. The dispatcher already has
//! a state machine for this kind of multi-turn capture
//! (`channel_automation_drafts`), so the keyboard renderer just produces the
//! force-reply payload — the dispatcher wires the "next reply" capture in W5.
//!
//! Telegram limits:
//! - `callback_data` ≤ 64 bytes per button.
//! - inline keyboard rows: no hard cap, but mobile clients render >2 rows
//!   poorly. We chunk at 3 per row.
//! - message text ≤ 4096 chars.

use serde_json::{json, Value};

use crate::traits::{InteractiveCard, InteractiveCardButton, InteractiveCardButtonStyle};

/// Build a `sendMessage` payload that delivers an interactive approval card.
///
/// The card title is rendered as a bold first line, followed by the body
/// markdown. Fields render as a key/value list. Buttons live in the
/// `reply_markup.inline_keyboard` array.
pub fn build_send_message_payload(card: &InteractiveCard) -> Value {
    json!({
        "chat_id": card.recipient,
        "text": render_message_text(card),
        "parse_mode": "MarkdownV2",
        "reply_markup": {
            "inline_keyboard": render_inline_keyboard(&card.correlation, &card.buttons),
        },
    })
}

/// Build an `editMessageText` payload that replaces the message body and
/// clears the inline keyboard. Use this after a decision lands.
pub fn build_edit_message_text_for_decision(
    card: &InteractiveCard,
    message_id: i64,
    decided_by_display: &str,
    decision_summary_markdown: &str,
) -> Value {
    let text = format!(
        "{}\n\n_{}_\n\n{}",
        escape_markdown_v2(&card.title),
        escape_markdown_v2(decided_by_display),
        escape_markdown_v2(decision_summary_markdown),
    );
    json!({
        "chat_id": card.recipient,
        "message_id": message_id,
        "text": text,
        "parse_mode": "MarkdownV2",
        "reply_markup": { "inline_keyboard": [] },
    })
}

/// Build an `editMessageReplyMarkup` payload that just removes the inline
/// keyboard, leaving the original message text intact. Useful for the
/// optimistic "I clicked, now hide buttons before the round-trip completes"
/// pattern.
pub fn build_clear_keyboard_payload(card: &InteractiveCard, message_id: i64) -> Value {
    json!({
        "chat_id": card.recipient,
        "message_id": message_id,
        "reply_markup": { "inline_keyboard": [] },
    })
}

/// Build a `sendMessage` payload that prompts the user to reply with a rework
/// reason. `force_reply: true` instructs Telegram clients to focus the input
/// box on the bot's message, and the dispatcher captures the user's NEXT
/// message as the rework feedback (W5 wiring).
///
/// `selective: true` ensures only the user who clicked Rework sees the prompt
/// in a group chat.
pub fn build_force_reply_for_rework(card: &InteractiveCard, mention_user_id: Option<i64>) -> Value {
    let prompt = card
        .reason_prompt
        .as_ref()
        .map(|p| p.field_label.clone())
        .unwrap_or_else(|| "What should change before this can be approved?".to_string());

    let mut payload = json!({
        "chat_id": card.recipient,
        "text": escape_markdown_v2(&prompt),
        "parse_mode": "MarkdownV2",
        "reply_markup": {
            "force_reply": true,
            "selective": true,
            "input_field_placeholder": card
                .reason_prompt
                .as_ref()
                .and_then(|p| p.field_placeholder.clone())
                .unwrap_or_else(|| "Type your rework feedback…".to_string()),
        },
    });

    // Tag the message with a reply-to so the dispatcher's session-aware
    // capture knows the user is mid-flow. Telegram's reply_parameters lets
    // us anchor to a specific source message; the dispatcher correlates
    // via the @-mention in groups.
    if let Some(user_id) = mention_user_id {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert(
                "text".to_string(),
                Value::String(format!("@user{} {}", user_id, escape_markdown_v2(&prompt))),
            );
        }
    }
    payload
}

fn render_message_text(card: &InteractiveCard) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("*{}*", escape_markdown_v2(&card.title)));
    if !card.body_markdown.trim().is_empty() {
        parts.push(escape_markdown_v2(&card.body_markdown));
    }
    if !card.fields.is_empty() {
        let lines: Vec<String> = card
            .fields
            .iter()
            .take(8)
            .map(|f| {
                format!(
                    "*{}:* {}",
                    escape_markdown_v2(&f.label),
                    escape_markdown_v2(&f.value)
                )
            })
            .collect();
        parts.push(lines.join("\n"));
    }
    let mut text = parts.join("\n\n");
    if text.chars().count() > 4096 {
        let mut truncated: String = text.chars().take(4095).collect();
        truncated.push('…');
        text = truncated;
    }
    text
}

fn render_inline_keyboard(
    correlation: &Value,
    buttons: &[InteractiveCardButton],
) -> Vec<Vec<Value>> {
    // Three buttons per row keeps mobile readability sane.
    buttons
        .chunks(3)
        .map(|row| {
            row.iter()
                .map(|btn| render_button(correlation, btn))
                .collect()
        })
        .collect()
}

fn render_button(correlation: &Value, btn: &InteractiveCardButton) -> Value {
    let callback_data = build_callback_data(correlation, &btn.action_id);
    json!({
        "text": prefix_for_style(btn.style, &btn.label),
        "callback_data": callback_data,
    })
}

/// Telegram doesn't have a button-style enum, so we use emoji prefixes to
/// signal intent visually: ✓ for primary, ↻ for default (rework), ✗ for
/// destructive. This keeps mobile UX legible without introducing custom
/// fonts.
fn prefix_for_style(style: InteractiveCardButtonStyle, label: &str) -> String {
    let prefix = match style {
        InteractiveCardButtonStyle::Primary => "✓ ",
        InteractiveCardButtonStyle::Destructive => "✗ ",
        InteractiveCardButtonStyle::Default => "",
    };
    let combined = format!("{prefix}{label}");
    // Telegram caps button labels at 64 bytes (UTF-8). Leave 3 bytes of slack
    // for any future emoji additions.
    if combined.len() <= 60 {
        combined
    } else {
        let mut truncated = String::with_capacity(60);
        for ch in combined.chars() {
            if truncated.len() + ch.len_utf8() > 59 {
                break;
            }
            truncated.push(ch);
        }
        truncated.push('…');
        truncated
    }
}

/// Telegram `callback_data` is the round-trip identifier for a button click.
/// Hard-capped at 64 bytes UTF-8 — we can't use a JSON object like Slack/Discord.
/// Format: `tdm:{action}:{run_id_short}:{node_id_short}`.
///
/// Long run_ids are truncated; the dispatcher resolves the full ID via a
/// short-lived cache (W5 wiring) since Telegram sees only what we encode.
fn build_callback_data(correlation: &Value, action_id: &str) -> String {
    let run_id = correlation
        .pointer("/automation_v2_run_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let node_id = correlation
        .pointer("/node_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let raw = format!("tdm:{action_id}:{run_id}:{node_id}");
    if raw.len() <= 64 {
        return raw;
    }
    // Truncate from the right; the run_id has the most stable prefix bits so
    // the dispatcher can disambiguate via cache lookup.
    let mut truncated: String = raw.chars().take(63).collect();
    truncated.push('~'); // marker that this was truncated
    truncated
}

/// Parse a callback_data string produced by `build_callback_data`.
pub fn parse_callback_data(data: &str) -> Option<ParsedCallbackData> {
    let trimmed = data.trim_end_matches('~');
    let was_truncated = trimmed.len() < data.len();
    let mut parts = trimmed.splitn(4, ':');
    let prefix = parts.next()?;
    if prefix != "tdm" {
        return None;
    }
    let action = parts.next()?;
    let run_id = parts.next()?;
    let node_id = parts.next().unwrap_or("");
    Some(ParsedCallbackData {
        action: action.to_string(),
        run_id: run_id.to_string(),
        node_id: node_id.to_string(),
        was_truncated,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCallbackData {
    pub action: String,
    pub run_id: String,
    pub node_id: String,
    /// True if the callback_data exceeded 64 bytes and was truncated. The
    /// dispatcher must resolve the full identifier via its short-lived cache.
    pub was_truncated: bool,
}

/// Escape Telegram MarkdownV2 metacharacters per
/// <https://core.telegram.org/bots/api#markdownv2-style>.
///
/// In MarkdownV2 the following must be escaped *outside* code/pre blocks:
/// `_ * [ ] ( ) ~ \` > # + - = | { } . !`
///
/// We do not interpret user content as actual markdown — we treat all input
/// as literal text and escape every metacharacter so nothing renders as
/// formatting accidentally.
fn escape_markdown_v2(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + input.len() / 4);
    for ch in input.chars() {
        match ch {
            '_' | '*' | '[' | ']' | '(' | ')' | '~' | '`' | '>' | '#' | '+' | '-' | '=' | '|'
            | '{' | '}' | '.' | '!' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{
        InteractiveCardButton, InteractiveCardButtonStyle, InteractiveCardField,
        InteractiveCardReasonPrompt,
    };

    fn approval_card() -> InteractiveCard {
        InteractiveCard {
            recipient: "12345".to_string(),
            title: "sales-research-outreach: approve email".to_string(),
            body_markdown:
                "Will email alice@example.com with subject Quick question about your stack."
                    .to_string(),
            fields: vec![
                InteractiveCardField {
                    label: "Run".to_string(),
                    value: "auto-v2-run-abc123".to_string(),
                },
                InteractiveCardField {
                    label: "Workflow".to_string(),
                    value: "sales-research-outreach".to_string(),
                },
            ],
            buttons: vec![
                InteractiveCardButton {
                    action_id: "approve".to_string(),
                    label: "Approve".to_string(),
                    style: InteractiveCardButtonStyle::Primary,
                    requires_reason: false,
                    confirm: None,
                },
                InteractiveCardButton {
                    action_id: "rework".to_string(),
                    label: "Rework".to_string(),
                    style: InteractiveCardButtonStyle::Default,
                    requires_reason: true,
                    confirm: None,
                },
                InteractiveCardButton {
                    action_id: "cancel".to_string(),
                    label: "Cancel".to_string(),
                    style: InteractiveCardButtonStyle::Destructive,
                    requires_reason: false,
                    confirm: None,
                },
            ],
            reason_prompt: Some(InteractiveCardReasonPrompt {
                modal_title: "Rework feedback".to_string(),
                field_label: "What should change?".to_string(),
                field_placeholder: Some("Tighten the ICP filter…".to_string()),
                submit_label: "Send back".to_string(),
            }),
            thread_key: None,
            correlation: json!({
                "automation_v2_run_id": "auto-v2-run-abc123",
                "node_id": "send_email",
            }),
        }
    }

    #[test]
    fn send_message_payload_uses_chat_id_and_markdownv2() {
        let payload = build_send_message_payload(&approval_card());
        assert_eq!(
            payload.get("chat_id").and_then(Value::as_str),
            Some("12345")
        );
        assert_eq!(
            payload.get("parse_mode").and_then(Value::as_str),
            Some("MarkdownV2")
        );
    }

    #[test]
    fn send_message_text_includes_title_and_body() {
        let payload = build_send_message_payload(&approval_card());
        let text = payload.get("text").and_then(Value::as_str).unwrap();
        assert!(text.contains("sales\\-research\\-outreach"));
        assert!(text.contains("alice@example\\.com"));
    }

    #[test]
    fn send_message_text_renders_fields_as_key_value_list() {
        let payload = build_send_message_payload(&approval_card());
        let text = payload.get("text").and_then(Value::as_str).unwrap();
        assert!(text.contains("*Run:*"));
        assert!(text.contains("auto\\-v2\\-run\\-abc123"));
        assert!(text.contains("*Workflow:*"));
    }

    #[test]
    fn inline_keyboard_renders_three_buttons_in_one_row() {
        let payload = build_send_message_payload(&approval_card());
        let keyboard = payload
            .pointer("/reply_markup/inline_keyboard")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(keyboard.len(), 1, "3 buttons fit one row at 3-per-row cap");
        let row = keyboard[0].as_array().unwrap();
        assert_eq!(row.len(), 3);
    }

    #[test]
    fn primary_button_renders_check_prefix() {
        let payload = build_send_message_payload(&approval_card());
        let approve = payload
            .pointer("/reply_markup/inline_keyboard/0/0")
            .unwrap();
        let label = approve.get("text").and_then(Value::as_str).unwrap();
        assert!(label.starts_with("✓ "));
        assert!(label.contains("Approve"));
    }

    #[test]
    fn destructive_button_renders_x_prefix() {
        let payload = build_send_message_payload(&approval_card());
        let cancel = payload
            .pointer("/reply_markup/inline_keyboard/0/2")
            .unwrap();
        let label = cancel.get("text").and_then(Value::as_str).unwrap();
        assert!(label.starts_with("✗ "));
    }

    #[test]
    fn callback_data_is_under_64_bytes() {
        let payload = build_send_message_payload(&approval_card());
        let approve = payload
            .pointer("/reply_markup/inline_keyboard/0/0")
            .unwrap();
        let cb = approve
            .get("callback_data")
            .and_then(Value::as_str)
            .unwrap();
        assert!(cb.len() <= 64, "callback_data must respect Telegram limit");
    }

    #[test]
    fn callback_data_round_trips_action_run_node() {
        let payload = build_send_message_payload(&approval_card());
        let approve = payload
            .pointer("/reply_markup/inline_keyboard/0/0")
            .unwrap();
        let cb = approve
            .get("callback_data")
            .and_then(Value::as_str)
            .unwrap();
        let parsed = parse_callback_data(cb).expect("parses");
        assert_eq!(parsed.action, "approve");
        assert_eq!(parsed.run_id, "auto-v2-run-abc123");
        assert_eq!(parsed.node_id, "send_email");
        assert!(!parsed.was_truncated);
    }

    #[test]
    fn long_callback_data_is_truncated_with_marker() {
        let mut card = approval_card();
        card.correlation = json!({
            "automation_v2_run_id": "very-long-run-id-with-many-characters-that-exceeds-the-budget-zzz",
            "node_id": "and-an-equally-long-node-id-that-also-eats-bytes",
        });
        let payload = build_send_message_payload(&card);
        let approve = payload
            .pointer("/reply_markup/inline_keyboard/0/0")
            .unwrap();
        let cb = approve
            .get("callback_data")
            .and_then(Value::as_str)
            .unwrap();
        assert!(cb.len() <= 64);
        assert!(cb.ends_with('~'));
        let parsed = parse_callback_data(cb).expect("parses");
        assert!(parsed.was_truncated);
    }

    #[test]
    fn parse_callback_data_rejects_unknown_prefix() {
        assert!(parse_callback_data("other:approve:run:node").is_none());
        assert!(parse_callback_data("notdm").is_none());
    }

    #[test]
    fn edit_message_text_for_decision_clears_keyboard() {
        let payload = build_edit_message_text_for_decision(
            &approval_card(),
            12345,
            "Approved by @alice at 14:32",
            "Outbound email sent.",
        );
        let keyboard = payload
            .pointer("/reply_markup/inline_keyboard")
            .and_then(Value::as_array)
            .unwrap();
        assert!(keyboard.is_empty());
        let text = payload.get("text").and_then(Value::as_str).unwrap();
        assert!(text.contains("Approved by @alice"));
        assert!(text.contains("Outbound email sent"));
    }

    #[test]
    fn clear_keyboard_payload_keeps_message_id() {
        let payload = build_clear_keyboard_payload(&approval_card(), 99);
        assert_eq!(payload.get("message_id").and_then(Value::as_i64), Some(99));
        assert!(
            payload.get("text").is_none(),
            "this endpoint should not retext"
        );
    }

    #[test]
    fn force_reply_for_rework_uses_label_from_prompt() {
        let payload = build_force_reply_for_rework(&approval_card(), None);
        assert_eq!(
            payload
                .pointer("/reply_markup/force_reply")
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            payload
                .pointer("/reply_markup/selective")
                .and_then(Value::as_bool),
            Some(true)
        );
        let placeholder = payload
            .pointer("/reply_markup/input_field_placeholder")
            .and_then(Value::as_str)
            .unwrap();
        assert_eq!(placeholder, "Tighten the ICP filter…");
    }

    #[test]
    fn force_reply_falls_back_when_no_reason_prompt() {
        let mut card = approval_card();
        card.reason_prompt = None;
        let payload = build_force_reply_for_rework(&card, None);
        let placeholder = payload
            .pointer("/reply_markup/input_field_placeholder")
            .and_then(Value::as_str)
            .unwrap();
        assert!(!placeholder.is_empty());
    }

    #[test]
    fn escape_markdown_v2_handles_telegram_metacharacters() {
        assert_eq!(escape_markdown_v2("_*[]"), "\\_\\*\\[\\]");
        assert_eq!(escape_markdown_v2("a.b!c"), "a\\.b\\!c");
        assert_eq!(escape_markdown_v2("hello world"), "hello world");
        assert_eq!(escape_markdown_v2("backslash\\here"), "backslash\\\\here");
    }

    #[test]
    fn long_button_label_is_truncated_under_64_bytes() {
        let long_label: String = "verbose ".repeat(20);
        let prefixed = prefix_for_style(InteractiveCardButtonStyle::Primary, &long_label);
        assert!(prefixed.len() <= 64);
    }
}
