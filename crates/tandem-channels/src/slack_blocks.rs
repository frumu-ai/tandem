//! Slack Block Kit renderer for [`InteractiveCard`].
//!
//! Pure functions: take an [`InteractiveCard`] in, produce a `serde_json::Value`
//! suitable for `chat.postMessage` / `chat.update` / `views.publish`. No I/O.
//! This makes the renderer trivially golden-testable.
//!
//! Block Kit reference: <https://api.slack.com/block-kit>.
//!
//! # Layout
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │  Header — `card.title`              │
//! │  Context — small line: run / req-by │
//! │  Section — `body_markdown`          │
//! │  Section — fields (key/value rows)  │
//! │  Divider                            │
//! │  Actions — buttons                  │
//! │    [Approve] [Rework] [Cancel] ⋮   │
//! └─────────────────────────────────────┘
//! ```

use serde_json::{json, Value};

use crate::traits::{
    InteractiveCard, InteractiveCardButton, InteractiveCardButtonStyle, InteractiveCardField,
    InteractiveCardReasonPrompt,
};

/// Render an [`InteractiveCard`] to a Block Kit `blocks` array.
///
/// The returned `Value` is the `blocks` field; callers wrap it in a full
/// `chat.postMessage` payload (adding `channel`, `text` fallback, optional
/// `thread_ts`).
pub fn render_card_blocks(card: &InteractiveCard) -> Value {
    let mut blocks: Vec<Value> = Vec::new();

    blocks.push(header_block(&card.title));

    if let Some(context) = context_block_from_fields(&card.fields) {
        blocks.push(context);
    }

    if !card.body_markdown.trim().is_empty() {
        blocks.push(body_section(&card.body_markdown));
    }

    if !card.fields.is_empty() {
        blocks.push(fields_section(&card.fields));
    }

    if !card.buttons.is_empty() {
        blocks.push(divider());
        for row in chunk_buttons_for_slack(&card.buttons) {
            blocks.push(actions_block(&card.correlation, row));
        }
    }

    Value::Array(blocks)
}

/// Build a complete `chat.postMessage` body, ready to POST to the Slack Web API.
///
/// `text` is a plain-text fallback shown in notification previews and screen
/// readers. Slack requires this even when blocks are provided.
pub fn build_post_message_payload(
    card: &InteractiveCard,
    text_fallback: &str,
    thread_ts: Option<&str>,
) -> Value {
    let mut payload = json!({
        "channel": card.recipient,
        "text": text_fallback,
        "blocks": render_card_blocks(card),
    });
    if let Some(ts) = thread_ts {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("thread_ts".to_string(), Value::String(ts.to_string()));
        }
    }
    payload
}

/// Build a `chat.update` body that replaces an existing card with a finalized
/// "decided" view: the original title, a context line showing who decided and
/// when, and a single grayed-out section showing the final outcome.
///
/// `decided_by_display` is what to show in the new context line ("approved by
/// @alice at 14:32"). `decision_summary_markdown` is the section body — keep
/// it short.
pub fn build_chat_update_payload_for_decision(
    card: &InteractiveCard,
    message_ts: &str,
    decided_by_display: &str,
    decision_summary_markdown: &str,
) -> Value {
    let blocks = vec![
        header_block(&card.title),
        json!({
            "type": "context",
            "elements": [
                { "type": "mrkdwn", "text": decided_by_display }
            ]
        }),
        json!({
            "type": "section",
            "text": { "type": "mrkdwn", "text": decision_summary_markdown }
        }),
    ];

    json!({
        "channel": card.recipient,
        "ts": message_ts,
        "text": decided_by_display,
        "blocks": blocks,
    })
}

/// Build a `views.open` modal payload for the rework-reason flow. Slack passes
/// the trigger_id back from the user's button click; the caller supplies it
/// here. `private_metadata` is an opaque string Slack echoes back verbatim in
/// the `view_submission` callback — the caller stashes its correlation there
/// (run id, channel binding) so the submission can be routed without any
/// server-side modal state.
pub fn build_rework_modal_payload(
    prompt: &InteractiveCardReasonPrompt,
    trigger_id: &str,
    callback_id: &str,
    private_metadata: &str,
) -> Value {
    json!({
        "trigger_id": trigger_id,
        "view": {
            "type": "modal",
            "callback_id": callback_id,
            "private_metadata": private_metadata,
            "title": { "type": "plain_text", "text": prompt.modal_title.clone() },
            "submit": { "type": "plain_text", "text": prompt.submit_label.clone() },
            "close": { "type": "plain_text", "text": "Cancel" },
            "blocks": [
                {
                    "type": "input",
                    "block_id": "reason_block",
                    "label": { "type": "plain_text", "text": prompt.field_label.clone() },
                    "element": {
                        "type": "plain_text_input",
                        "action_id": "reason_input",
                        "multiline": true,
                        "placeholder": prompt.field_placeholder.as_ref().map(|p| json!({
                            "type": "plain_text",
                            "text": p
                        })).unwrap_or(json!(null))
                    }
                }
            ]
        }
    })
}

fn header_block(title: &str) -> Value {
    json!({
        "type": "header",
        "text": {
            "type": "plain_text",
            "text": clamp_plain_text(title, 150),
            "emoji": true
        }
    })
}

fn context_block_from_fields(fields: &[InteractiveCardField]) -> Option<Value> {
    // Slack context blocks render small text; we only render the first 1–2
    // critical identity fields here so the header area stays scannable. The
    // remaining fields show in the section grid below.
    let context_labels = ["Run", "Requested by", "Workflow"];
    let elements: Vec<Value> = fields
        .iter()
        .filter(|f| context_labels.iter().any(|label| f.label == *label))
        .take(3)
        .map(|f| {
            json!({
                "type": "mrkdwn",
                "text": format!("*{}:* {}", escape_mrkdwn(&f.label), escape_mrkdwn(&f.value))
            })
        })
        .collect();
    if elements.is_empty() {
        None
    } else {
        Some(json!({ "type": "context", "elements": elements }))
    }
}

fn body_section(markdown: &str) -> Value {
    json!({
        "type": "section",
        "text": {
            "type": "mrkdwn",
            "text": clamp_plain_text(markdown, 3000)
        }
    })
}

fn fields_section(fields: &[InteractiveCardField]) -> Value {
    // Slack section `fields` arrays render as a 2-column grid. Each entry caps
    // at 2000 chars; we get up to 10 fields.
    let elements: Vec<Value> = fields
        .iter()
        .take(10)
        .map(|f| {
            json!({
                "type": "mrkdwn",
                "text": format!(
                    "*{}*\n{}",
                    escape_mrkdwn(&f.label),
                    clamp_plain_text(&escape_mrkdwn(&f.value), 1900)
                )
            })
        })
        .collect();
    json!({ "type": "section", "fields": elements })
}

fn divider() -> Value {
    json!({ "type": "divider" })
}

fn actions_block(correlation: &Value, buttons: &[InteractiveCardButton]) -> Value {
    let elements: Vec<Value> = buttons
        .iter()
        .map(|btn| render_button(correlation, btn))
        .collect();
    json!({
        "type": "actions",
        "elements": elements,
    })
}

fn render_button(correlation: &Value, btn: &InteractiveCardButton) -> Value {
    let value = json!({
        "action_id": btn.action_id,
        "requires_reason": btn.requires_reason,
        "correlation": correlation,
    })
    .to_string();

    let mut element = json!({
        "type": "button",
        "action_id": btn.action_id,
        "text": {
            "type": "plain_text",
            "text": clamp_plain_text(&btn.label, 75),
            "emoji": true
        },
        "value": clamp_plain_text(&value, 2000),
    });

    if let Some(style) = button_style_to_slack(btn.style) {
        if let Some(obj) = element.as_object_mut() {
            obj.insert("style".to_string(), Value::String(style.to_string()));
        }
    }

    if let Some(confirm) = &btn.confirm {
        if let Some(obj) = element.as_object_mut() {
            obj.insert(
                "confirm".to_string(),
                json!({
                    "title": { "type": "plain_text", "text": clamp_plain_text(&confirm.title, 100) },
                    "text": { "type": "mrkdwn", "text": clamp_plain_text(&confirm.body, 300) },
                    "confirm": { "type": "plain_text", "text": clamp_plain_text(&confirm.confirm_label, 30) },
                    "deny": { "type": "plain_text", "text": clamp_plain_text(&confirm.deny_label, 30) }
                }),
            );
        }
    }

    element
}

fn button_style_to_slack(style: InteractiveCardButtonStyle) -> Option<&'static str> {
    match style {
        InteractiveCardButtonStyle::Default => None,
        InteractiveCardButtonStyle::Primary => Some("primary"),
        InteractiveCardButtonStyle::Destructive => Some("danger"),
    }
}

/// Slack action rows cap at 5 elements per actions block; chunk if more.
fn chunk_buttons_for_slack(buttons: &[InteractiveCardButton]) -> Vec<&[InteractiveCardButton]> {
    buttons.chunks(5).collect()
}

fn clamp_plain_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        input.to_string()
    } else {
        let mut out: String = input.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Escape Slack mrkdwn metacharacters that would otherwise format text. We
/// intentionally do not run user-controlled markdown through this — the
/// `body_markdown` field is trusted formatted text from the workflow itself.
/// This helper exists for *labels* and *field values* that come from arbitrary
/// data and should render literally.
fn escape_mrkdwn(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{
        InteractiveCardButton, InteractiveCardButtonStyle, InteractiveCardConfirm,
        InteractiveCardField, InteractiveCardReasonPrompt,
    };

    fn approval_card() -> InteractiveCard {
        InteractiveCard {
            recipient: "C12345678".to_string(),
            title: "sales-research-outreach · approve outbound email".to_string(),
            body_markdown:
                "Will email *alice@example.com* with subject _Quick question about your stack_."
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
                InteractiveCardField {
                    label: "Recipient".to_string(),
                    value: "alice@example.com".to_string(),
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
                    confirm: Some(InteractiveCardConfirm {
                        title: "Cancel run?".to_string(),
                        body: "This stops the workflow run and cannot be resumed.".to_string(),
                        confirm_label: "Cancel run".to_string(),
                        deny_label: "Keep waiting".to_string(),
                    }),
                },
            ],
            reason_prompt: Some(InteractiveCardReasonPrompt {
                modal_title: "Rework feedback".to_string(),
                field_label: "What should change?".to_string(),
                field_placeholder: Some("Tighten the ICP filter…".to_string()),
                submit_label: "Send back".to_string(),
            }),
            thread_key: Some("run-thread-abc123".to_string()),
            correlation: json!({
                "automation_v2_run_id": "auto-v2-run-abc123",
                "node_id": "send_email",
                "request_id": "automation_v2:auto-v2-run-abc123:send_email"
            }),
        }
    }

    #[test]
    fn renders_header_with_title() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().expect("blocks array");
        let header = &arr[0];
        assert_eq!(header.get("type").and_then(Value::as_str), Some("header"));
        assert_eq!(
            header.pointer("/text/text").and_then(Value::as_str),
            Some("sales-research-outreach · approve outbound email")
        );
    }

    #[test]
    fn renders_context_block_for_identity_fields() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().unwrap();
        let context = arr
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("context"))
            .expect("context block");
        let elements = context
            .get("elements")
            .and_then(Value::as_array)
            .expect("elements array");
        // Only "Run" and "Workflow" qualify; "Recipient" goes in the fields grid.
        assert_eq!(elements.len(), 2);
        let texts: Vec<&str> = elements
            .iter()
            .filter_map(|e| e.get("text").and_then(Value::as_str))
            .collect();
        assert!(texts.iter().any(|t| t.contains("auto-v2-run-abc123")));
        assert!(texts.iter().any(|t| t.contains("sales-research-outreach")));
    }

    #[test]
    fn renders_body_section() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().unwrap();
        let body = arr
            .iter()
            .find(|b| {
                b.get("type").and_then(Value::as_str) == Some("section")
                    && b.pointer("/text/text").is_some()
            })
            .expect("body section");
        let text = body.pointer("/text/text").and_then(Value::as_str).unwrap();
        assert!(text.contains("alice@example.com"));
    }

    #[test]
    fn renders_fields_grid_with_all_three_fields() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().unwrap();
        let fields = arr
            .iter()
            .find(|b| b.get("fields").is_some())
            .expect("fields section");
        let entries = fields.get("fields").and_then(Value::as_array).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn renders_actions_block_with_buttons() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().unwrap();
        let actions = arr
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("actions"))
            .expect("actions block");
        let elements = actions.get("elements").and_then(Value::as_array).unwrap();
        assert_eq!(elements.len(), 3);
        let action_ids: Vec<&str> = elements
            .iter()
            .filter_map(|e| e.get("action_id").and_then(Value::as_str))
            .collect();
        assert_eq!(action_ids, vec!["approve", "rework", "cancel"]);
    }

    #[test]
    fn primary_button_renders_style_primary() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().unwrap();
        let actions = arr
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("actions"))
            .unwrap();
        let elements = actions.get("elements").and_then(Value::as_array).unwrap();
        let approve = &elements[0];
        assert_eq!(
            approve.get("style").and_then(Value::as_str),
            Some("primary")
        );
    }

    #[test]
    fn destructive_button_renders_style_danger_and_confirm() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().unwrap();
        let actions = arr
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("actions"))
            .unwrap();
        let elements = actions.get("elements").and_then(Value::as_array).unwrap();
        let cancel = &elements[2];
        assert_eq!(cancel.get("style").and_then(Value::as_str), Some("danger"));
        let confirm = cancel.get("confirm").expect("confirm dialog");
        assert_eq!(
            confirm.pointer("/title/text").and_then(Value::as_str),
            Some("Cancel run?")
        );
        assert_eq!(
            confirm.pointer("/confirm/text").and_then(Value::as_str),
            Some("Cancel run")
        );
    }

    #[test]
    fn default_style_button_omits_style_field() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().unwrap();
        let actions = arr
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("actions"))
            .unwrap();
        let elements = actions.get("elements").and_then(Value::as_array).unwrap();
        let rework = &elements[1];
        assert!(rework.get("style").is_none());
    }

    #[test]
    fn button_value_round_trips_action_and_correlation() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().unwrap();
        let actions = arr
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("actions"))
            .unwrap();
        let elements = actions.get("elements").and_then(Value::as_array).unwrap();
        let approve = &elements[0];
        let value_str = approve.get("value").and_then(Value::as_str).unwrap();
        let parsed: Value = serde_json::from_str(value_str).expect("button value is JSON");
        assert_eq!(
            parsed.get("action_id").and_then(Value::as_str),
            Some("approve")
        );
        assert_eq!(
            parsed
                .pointer("/correlation/automation_v2_run_id")
                .and_then(Value::as_str),
            Some("auto-v2-run-abc123")
        );
    }

    #[test]
    fn render_card_blocks_includes_divider_before_actions() {
        let card = approval_card();
        let blocks = render_card_blocks(&card);
        let arr = blocks.as_array().unwrap();
        let actions_idx = arr
            .iter()
            .position(|b| b.get("type").and_then(Value::as_str) == Some("actions"))
            .unwrap();
        let divider = &arr[actions_idx - 1];
        assert_eq!(divider.get("type").and_then(Value::as_str), Some("divider"));
    }

    #[test]
    fn build_post_message_payload_attaches_thread_ts_when_provided() {
        let card = approval_card();
        let payload =
            build_post_message_payload(&card, "approval needed", Some("1700000000.000100"));
        assert_eq!(
            payload.get("thread_ts").and_then(Value::as_str),
            Some("1700000000.000100")
        );
        assert_eq!(
            payload.get("channel").and_then(Value::as_str),
            Some("C12345678")
        );
        assert_eq!(
            payload.get("text").and_then(Value::as_str),
            Some("approval needed")
        );
        assert!(payload.get("blocks").is_some());
    }

    #[test]
    fn build_post_message_payload_omits_thread_ts_when_absent() {
        let card = approval_card();
        let payload = build_post_message_payload(&card, "approval needed", None);
        assert!(payload.get("thread_ts").is_none());
    }

    #[test]
    fn chunk_buttons_for_slack_caps_at_five_per_row() {
        let buttons: Vec<InteractiveCardButton> = (0..7)
            .map(|i| InteractiveCardButton {
                action_id: format!("a{i}"),
                label: format!("Button {i}"),
                style: InteractiveCardButtonStyle::Default,
                requires_reason: false,
                confirm: None,
            })
            .collect();
        let chunks = chunk_buttons_for_slack(&buttons);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 5);
        assert_eq!(chunks[1].len(), 2);
    }

    #[test]
    fn build_chat_update_payload_replaces_buttons_with_decision_summary() {
        let card = approval_card();
        let payload = build_chat_update_payload_for_decision(
            &card,
            "1700000000.000100",
            "Approved by @alice at 14:32",
            "Outbound email sent. <https://crm.example.com/activity/123|See CRM activity>.",
        );
        assert_eq!(
            payload.get("ts").and_then(Value::as_str),
            Some("1700000000.000100")
        );
        let blocks = payload
            .get("blocks")
            .and_then(Value::as_array)
            .expect("blocks");
        // Header + context + section, no actions block.
        assert_eq!(blocks.len(), 3);
        assert!(!blocks
            .iter()
            .any(|b| b.get("type").and_then(Value::as_str) == Some("actions")));
    }

    #[test]
    fn build_rework_modal_payload_includes_input_block_with_label() {
        let card = approval_card();
        let prompt = card.reason_prompt.as_ref().expect("reason prompt");
        let modal = build_rework_modal_payload(
            prompt,
            "trigger.123",
            "rework_modal_v1",
            "{\"automation_v2_run_id\":\"run-1\"}",
        );
        assert_eq!(
            modal.get("trigger_id").and_then(Value::as_str),
            Some("trigger.123")
        );
        let view = modal.get("view").expect("view");
        assert_eq!(view.get("type").and_then(Value::as_str), Some("modal"));
        assert_eq!(
            view.get("callback_id").and_then(Value::as_str),
            Some("rework_modal_v1")
        );
        assert_eq!(
            view.get("private_metadata").and_then(Value::as_str),
            Some("{\"automation_v2_run_id\":\"run-1\"}"),
            "private_metadata must round-trip through the view"
        );
        let blocks = view.get("blocks").and_then(Value::as_array).unwrap();
        let input_block = &blocks[0];
        assert_eq!(
            input_block.get("type").and_then(Value::as_str),
            Some("input")
        );
        assert_eq!(
            input_block.pointer("/label/text").and_then(Value::as_str),
            Some("What should change?")
        );
        assert_eq!(
            input_block
                .pointer("/element/action_id")
                .and_then(Value::as_str),
            Some("reason_input")
        );
    }

    #[test]
    fn clamp_plain_text_truncates_with_ellipsis() {
        let long: String = "x".repeat(200);
        let clamped = clamp_plain_text(&long, 50);
        assert_eq!(clamped.chars().count(), 50);
        assert!(clamped.ends_with('…'));
    }

    #[test]
    fn escape_mrkdwn_escapes_html_meta_characters() {
        assert_eq!(escape_mrkdwn("<a&b>"), "&lt;a&amp;b&gt;");
    }
}
